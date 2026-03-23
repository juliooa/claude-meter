use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use opentelemetry_proto::tonic::{
    collector::metrics::v1::{ExportMetricsServiceRequest, ExportMetricsServiceResponse},
    common::v1::{any_value, KeyValue},
    metrics::v1::metric::Data,
    metrics::v1::number_data_point,
};
use prost::Message;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::db;

struct AppState {
    db: Mutex<Connection>,
    /// Tracks last cumulative value per (session+metric+type+model) key
    /// for computing deltas from cumulative counters.
    last_cumulative: Mutex<HashMap<String, f64>>,
}

pub async fn run(port: u16, db_path: PathBuf) -> anyhow::Result<()> {
    let conn = db::open(&db_path)?;
    let state = Arc::new(AppState {
        db: Mutex::new(conn),
        last_cumulative: Mutex::new(HashMap::new()),
    });

    let app = Router::new()
        .route("/", get(health))
        .route("/v1/metrics", post(handle_metrics))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("claude-meter listening on {}", addr);
    println!("Database: {}", db_path.display());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> &'static str {
    "OK"
}

async fn handle_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Accept both protobuf and JSON content types, but only parse protobuf
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type.is_empty()
        && !content_type.contains("protobuf")
        && !content_type.contains("octet-stream")
    {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected application/x-protobuf",
        )
            .into_response();
    }

    let request = match ExportMetricsServiceRequest::decode(body.as_ref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to decode protobuf: {}", e);
            return (StatusCode::BAD_REQUEST, format!("Decode error: {}", e)).into_response();
        }
    };

    let rows = extract_rows(&state, &request);

    if !rows.is_empty() {
        let conn = state.db.lock().unwrap();
        for row in &rows {
            if let Err(e) = db::insert(&conn, row) {
                eprintln!("DB insert error: {}", e);
            }
        }
    }

    let response = ExportMetricsServiceResponse {
        partial_success: None,
    };
    let mut buf = Vec::new();
    response.encode(&mut buf).unwrap();

    (
        StatusCode::OK,
        [("content-type", "application/x-protobuf")],
        buf,
    )
        .into_response()
}

fn extract_rows(state: &AppState, request: &ExportMetricsServiceRequest) -> Vec<db::MetricRow> {
    let mut rows = Vec::new();

    for rm in &request.resource_metrics {
        let resource_session_id = rm
            .resource
            .as_ref()
            .and_then(|r| find_string_attr(&r.attributes, "session.id"));

        for sm in &rm.scope_metrics {
            for metric in &sm.metrics {
                let metric_name = &metric.name;

                match &metric.data {
                    Some(Data::Sum(sum)) => {
                        let is_cumulative = sum.aggregation_temporality == 2;
                        for dp in &sum.data_points {
                            let raw_value = extract_number_value(&dp.value);
                            let session_id = find_string_attr(&dp.attributes, "session.id")
                                .or_else(|| resource_session_id.clone());
                            let model = find_string_attr(&dp.attributes, "model");
                            let metric_type = find_string_attr(&dp.attributes, "type");
                            let tool_name = find_string_attr(&dp.attributes, "tool_name");
                            let decision = find_string_attr(&dp.attributes, "decision");
                            let timestamp = (dp.time_unix_nano / 1_000_000_000) as i64;

                            let value = if is_cumulative {
                                compute_delta(
                                    state,
                                    &session_id,
                                    metric_name,
                                    &metric_type,
                                    &model,
                                    raw_value,
                                )
                            } else {
                                raw_value
                            };

                            if value <= 0.0 {
                                continue;
                            }

                            let extra = collect_extra_attrs(
                                &dp.attributes,
                                &["session.id", "model", "type", "tool_name", "decision"],
                            );

                            rows.push(db::MetricRow {
                                timestamp,
                                metric_name: metric_name.clone(),
                                value,
                                session_id,
                                model,
                                metric_type,
                                tool_name,
                                decision,
                                attributes_json: extra,
                            });
                        }
                    }
                    Some(Data::Gauge(gauge)) => {
                        for dp in &gauge.data_points {
                            let value = extract_number_value(&dp.value);
                            let session_id = find_string_attr(&dp.attributes, "session.id")
                                .or_else(|| resource_session_id.clone());
                            let model = find_string_attr(&dp.attributes, "model");
                            let metric_type = find_string_attr(&dp.attributes, "type");
                            let tool_name = find_string_attr(&dp.attributes, "tool_name");
                            let decision = find_string_attr(&dp.attributes, "decision");
                            let timestamp = (dp.time_unix_nano / 1_000_000_000) as i64;

                            let extra = collect_extra_attrs(
                                &dp.attributes,
                                &["session.id", "model", "type", "tool_name", "decision"],
                            );

                            rows.push(db::MetricRow {
                                timestamp,
                                metric_name: metric_name.clone(),
                                value,
                                session_id,
                                model,
                                metric_type,
                                tool_name,
                                decision,
                                attributes_json: extra,
                            });
                        }
                    }
                    Some(Data::Histogram(hist)) => {
                        for dp in &hist.data_points {
                            let value = dp.sum.unwrap_or(0.0);
                            let session_id = find_string_attr(&dp.attributes, "session.id")
                                .or_else(|| resource_session_id.clone());
                            let model = find_string_attr(&dp.attributes, "model");
                            let metric_type = find_string_attr(&dp.attributes, "type");
                            let tool_name = find_string_attr(&dp.attributes, "tool_name");
                            let decision = find_string_attr(&dp.attributes, "decision");
                            let timestamp = (dp.time_unix_nano / 1_000_000_000) as i64;

                            let extra = collect_extra_attrs(
                                &dp.attributes,
                                &["session.id", "model", "type", "tool_name", "decision"],
                            );

                            rows.push(db::MetricRow {
                                timestamp,
                                metric_name: metric_name.clone(),
                                value,
                                session_id,
                                model,
                                metric_type,
                                tool_name,
                                decision,
                                attributes_json: extra,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    rows
}

fn compute_delta(
    state: &AppState,
    session_id: &Option<String>,
    metric_name: &str,
    metric_type: &Option<String>,
    model: &Option<String>,
    raw_value: f64,
) -> f64 {
    let key = format!(
        "{}|{}|{}|{}",
        session_id.as_deref().unwrap_or(""),
        metric_name,
        metric_type.as_deref().unwrap_or(""),
        model.as_deref().unwrap_or("")
    );

    let mut map = state.last_cumulative.lock().unwrap();
    let last = map.get(&key).copied().unwrap_or(0.0);

    if raw_value >= last {
        let delta = raw_value - last;
        map.insert(key, raw_value);
        delta
    } else {
        // Counter reset (new session or process restart)
        map.insert(key, raw_value);
        raw_value
    }
}

fn extract_number_value(value: &Option<number_data_point::Value>) -> f64 {
    match value {
        Some(number_data_point::Value::AsDouble(v)) => *v,
        Some(number_data_point::Value::AsInt(v)) => *v as f64,
        None => 0.0,
    }
}

fn find_string_attr(attrs: &[KeyValue], key: &str) -> Option<String> {
    attrs.iter().find(|kv| kv.key == key).and_then(|kv| {
        kv.value.as_ref().and_then(|v| match &v.value {
            Some(any_value::Value::StringValue(s)) => Some(s.clone()),
            Some(any_value::Value::IntValue(i)) => Some(i.to_string()),
            _ => None,
        })
    })
}

fn collect_extra_attrs(attrs: &[KeyValue], skip_keys: &[&str]) -> Option<String> {
    let extra: serde_json::Map<String, serde_json::Value> = attrs
        .iter()
        .filter(|kv| !skip_keys.contains(&kv.key.as_str()))
        .filter_map(|kv| {
            let val = kv.value.as_ref().and_then(|v| match &v.value {
                Some(any_value::Value::StringValue(s)) => {
                    Some(serde_json::Value::String(s.clone()))
                }
                Some(any_value::Value::IntValue(i)) => {
                    Some(serde_json::Value::Number((*i).into()))
                }
                Some(any_value::Value::DoubleValue(d)) => {
                    serde_json::Number::from_f64(*d).map(serde_json::Value::Number)
                }
                Some(any_value::Value::BoolValue(b)) => Some(serde_json::Value::Bool(*b)),
                _ => None,
            });
            val.map(|v| (kv.key.clone(), v))
        })
        .collect();

    if extra.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&extra).unwrap())
    }
}
