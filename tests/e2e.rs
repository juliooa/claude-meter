use opentelemetry_proto::tonic::{
    collector::metrics::v1::{ExportMetricsServiceRequest, ExportMetricsServiceResponse},
    common::v1::{any_value, AnyValue, KeyValue},
    metrics::v1::{
        metric, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum,
        number_data_point,
    },
    resource::v1::Resource,
};
use prost::Message;

fn kv(key: &str, val: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(val.to_string())),
        }),
    }
}

fn int_dp(value: i64, time_secs: u64, attrs: Vec<KeyValue>) -> NumberDataPoint {
    NumberDataPoint {
        attributes: attrs,
        time_unix_nano: time_secs * 1_000_000_000,
        start_time_unix_nano: (time_secs - 10) * 1_000_000_000,
        value: Some(number_data_point::Value::AsInt(value)),
        ..Default::default()
    }
}

fn double_dp(value: f64, time_secs: u64, attrs: Vec<KeyValue>) -> NumberDataPoint {
    NumberDataPoint {
        attributes: attrs,
        time_unix_nano: time_secs * 1_000_000_000,
        start_time_unix_nano: (time_secs - 10) * 1_000_000_000,
        value: Some(number_data_point::Value::AsDouble(value)),
        ..Default::default()
    }
}

#[tokio::test]
async fn test_otlp_ingest_and_query() {
    let db_path = "/tmp/test-e2e-claude-meter.db";
    let _ = std::fs::remove_file(db_path);
    let port = 14319u16;

    // Start server in background
    let db_pathbuf = std::path::PathBuf::from(db_path);
    let server_handle = tokio::spawn(async move {
        claude_meter::server::run(port, db_pathbuf).await.unwrap();
    });

    // Wait for server to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Build a realistic OTLP request with token, cost, and line metrics
    let request = ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: Some(Resource {
                attributes: vec![kv("session.id", "test-session-abc123")],
                dropped_attributes_count: 0,
            }),
            scope_metrics: vec![ScopeMetrics {
                scope: None,
                metrics: vec![
                    Metric {
                        name: "claude_code.token.usage".to_string(),
                        description: "Token usage".to_string(),
                        unit: "tokens".to_string(),
                        metadata: vec![],
                        data: Some(metric::Data::Sum(Sum {
                            data_points: vec![
                                int_dp(
                                    12450,
                                    now,
                                    vec![
                                        kv("type", "input"),
                                        kv("model", "claude-opus-4-6"),
                                    ],
                                ),
                                int_dp(
                                    3200,
                                    now,
                                    vec![
                                        kv("type", "output"),
                                        kv("model", "claude-opus-4-6"),
                                    ],
                                ),
                                int_dp(
                                    45000,
                                    now,
                                    vec![
                                        kv("type", "cacheRead"),
                                        kv("model", "claude-opus-4-6"),
                                    ],
                                ),
                                int_dp(
                                    8100,
                                    now,
                                    vec![
                                        kv("type", "cacheCreation"),
                                        kv("model", "claude-opus-4-6"),
                                    ],
                                ),
                            ],
                            aggregation_temporality: 1, // DELTA
                            is_monotonic: true,
                        })),
                    },
                    Metric {
                        name: "claude_code.cost.usage".to_string(),
                        description: "Cost".to_string(),
                        unit: "usd".to_string(),
                        metadata: vec![],
                        data: Some(metric::Data::Sum(Sum {
                            data_points: vec![double_dp(
                                0.42,
                                now,
                                vec![kv("model", "claude-opus-4-6")],
                            )],
                            aggregation_temporality: 1,
                            is_monotonic: true,
                        })),
                    },
                    Metric {
                        name: "claude_code.lines_of_code.modified".to_string(),
                        description: "Lines modified".to_string(),
                        unit: "lines".to_string(),
                        metadata: vec![],
                        data: Some(metric::Data::Sum(Sum {
                            data_points: vec![
                                int_dp(156, now, vec![kv("type", "added")]),
                                int_dp(34, now, vec![kv("type", "removed")]),
                            ],
                            aggregation_temporality: 1,
                            is_monotonic: true,
                        })),
                    },
                ],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    // Encode and send
    let mut buf = Vec::new();
    request.encode(&mut buf).unwrap();

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://localhost:{}/v1/metrics", port))
        .header("content-type", "application/x-protobuf")
        .body(buf)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 200);

    // Decode the response
    let resp_bytes = resp.bytes().await.unwrap();
    let _resp = ExportMetricsServiceResponse::decode(resp_bytes.as_ref()).unwrap();

    // Query the DB directly
    let conn = claude_meter::db::open(std::path::Path::new(db_path)).unwrap();
    let usage = claude_meter::db::query_usage(&conn, 0).unwrap();

    assert_eq!(usage.input_tokens as i64, 12450);
    assert_eq!(usage.output_tokens as i64, 3200);
    assert_eq!(usage.cache_read_tokens as i64, 45000);
    assert_eq!(usage.cache_creation_tokens as i64, 8100);
    assert!((usage.cost - 0.42).abs() < 0.001);
    assert_eq!(usage.sessions, 1);
    assert_eq!(usage.lines_added as i64, 156);
    assert_eq!(usage.lines_removed as i64, 34);

    // Check model breakdown
    let models = claude_meter::db::query_by_model(&conn).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model, "claude-opus-4-6");
    assert_eq!(models[0].input_tokens as i64, 12450);

    // Check session breakdown
    let sessions = claude_meter::db::query_by_session(&conn, 0).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].session_id.contains("test-session"));

    // Cleanup
    server_handle.abort();
    let _ = std::fs::remove_file(db_path);

    println!("All assertions passed!");
}
