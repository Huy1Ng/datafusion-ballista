// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License.

use crate::config::BallistaConfig;
use crate::error::{BallistaError, Result};
use crate::extension::SessionConfigExt;
use crate::serde::scheduler::PartitionStats;

use datafusion::arrow::array::ArrayRef;
use datafusion::arrow::array::cast::AsArray;
use datafusion::arrow::datatypes::DataType;
use datafusion::arrow::ipc::CompressionType;
use datafusion::arrow::ipc::writer::IpcWriteOptions;
use datafusion::arrow::ipc::writer::StreamWriter;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::DataFusionError;
use datafusion::execution::context::{SessionConfig, SessionState};
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::execution::session_state::SessionStateBuilder;
use datafusion::physical_plan::metrics::MetricsSet;
use datafusion::physical_plan::{ExecutionPlan, RecordBatchStream, metrics};
use futures::StreamExt;
use log::error;
use std::io::BufWriter;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs::File, pin::Pin};
use tonic::codegen::StdError;
use tonic::transport::{Channel, Endpoint, Error, Server};

/// Configuration for gRPC client connections.
///
/// This struct holds timeout and keep-alive settings that are applied
/// when establishing gRPC connections from executors to schedulers or
/// between distributed components.
///
/// # Examples
///
/// ```
/// use ballista_core::config::BallistaConfig;
/// use ballista_core::utils::GrpcClientConfig;
///
/// let ballista_config = BallistaConfig::default();
/// let grpc_config = GrpcClientConfig::from(&ballista_config);
/// ```
#[derive(Debug, Clone)]
pub struct GrpcClientConfig {
    /// Connection timeout in seconds
    pub connect_timeout_seconds: u64,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// TCP keep-alive interval in seconds
    pub tcp_keepalive_seconds: u64,
    /// HTTP/2 keep-alive ping interval in seconds
    pub http2_keepalive_interval_seconds: u64,
}

impl From<&BallistaConfig> for GrpcClientConfig {
    fn from(config: &BallistaConfig) -> Self {
        Self {
            connect_timeout_seconds: config.default_grpc_client_connect_timeout_seconds()
                as u64,
            timeout_seconds: config.default_grpc_client_timeout_seconds() as u64,
            tcp_keepalive_seconds: config.default_grpc_client_tcp_keepalive_seconds()
                as u64,
            http2_keepalive_interval_seconds: config
                .default_grpc_client_http2_keepalive_interval_seconds()
                as u64,
        }
    }
}

impl Default for GrpcClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: 20,
            timeout_seconds: 20,
            tcp_keepalive_seconds: 3600,
            http2_keepalive_interval_seconds: 300,
        }
    }
}

/// Configuration for gRPC server.
///
/// This struct holds timeout and keep-alive settings that are applied
/// when creating gRPC servers in executors and schedulers.
///
/// # Examples
///
/// ```
/// use ballista_core::utils::GrpcServerConfig;
///
/// let server_config = GrpcServerConfig::default();
/// let server = ballista_core::utils::create_grpc_server(&server_config);
/// ```
#[derive(Debug, Clone)]
pub struct GrpcServerConfig {
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// TCP keep-alive interval in seconds
    pub tcp_keepalive_seconds: u64,
    /// HTTP/2 keep-alive ping interval in seconds
    pub http2_keepalive_interval_seconds: u64,
    /// HTTP/2 keep-alive ping timeout in seconds
    pub http2_keepalive_timeout_seconds: u64,
}

impl Default for GrpcServerConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 20,
            tcp_keepalive_seconds: 3600,
            http2_keepalive_interval_seconds: 300,
            http2_keepalive_timeout_seconds: 20,
        }
    }
}

/// Default session builder using the provided configuration
pub fn default_session_builder(
    config: SessionConfig,
) -> datafusion::common::Result<SessionState> {
    Ok(SessionStateBuilder::new()
        .with_default_features()
        .with_config(config)
        .with_runtime_env(Arc::new(RuntimeEnvBuilder::new().build()?))
        .build())
}

/// Creates a default session configuration with Ballista extensions.
pub fn default_config_producer() -> SessionConfig {
    SessionConfig::new_with_ballista()
}

/// Stream data to disk in Arrow IPC format
pub async fn write_stream_to_disk(
    stream: &mut Pin<Box<dyn RecordBatchStream + Send>>,
    path: &str,
    disk_write_metric: &metrics::Time,
    max_grpc_message_size: usize,
) -> Result<PartitionStats> {
    let file = BufWriter::new(File::create(path).map_err(|e| {
        error!("Failed to create partition file at {path}: {e:?}");
        BallistaError::IoError(e)
    })?);

    let mut num_rows = 0;
    let mut num_batches = 0;
    let mut num_bytes = 0;

    let options = IpcWriteOptions::default()
        .try_with_compression(Some(CompressionType::LZ4_FRAME))?;

    let mut writer =
        StreamWriter::try_new_with_options(file, stream.schema().as_ref(), options)?;

    while let Some(result) = stream.next().await {
        let batch = result?;
        let compacted_batch = maybe_gc_batch(batch, max_grpc_message_size)?;
        let batch_size_bytes: usize = compacted_batch.get_array_memory_size();
        num_batches += 1;
        num_rows += compacted_batch.num_rows();
        num_bytes += batch_size_bytes;

        let timer = disk_write_metric.timer();
        writer.write(&compacted_batch)?;
        timer.done();
    }
    let timer = disk_write_metric.timer();
    writer.finish()?;
    timer.done();
    Ok(PartitionStats::new(
        Some(num_rows as u64),
        Some(num_batches),
        Some(num_bytes as u64),
    ))
}

/// Collects all record batches from a stream into a vector.
pub async fn collect_stream(
    stream: &mut Pin<Box<dyn RecordBatchStream + Send>>,
) -> Result<Vec<RecordBatch>> {
    let mut batches = vec![];
    while let Some(batch) = stream.next().await {
        batches.push(batch?);
    }
    Ok(batches)
}

/// Creates a gRPC client connection with the specified configuration.
pub async fn create_grpc_client_connection<D>(
    dst: D,
    config: &GrpcClientConfig,
) -> std::result::Result<Channel, Error>
where
    D: std::convert::TryInto<tonic::transport::Endpoint>,
    D::Error: Into<StdError>,
{
    let endpoint = tonic::transport::Endpoint::new(dst)?
        .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
        .timeout(Duration::from_secs(config.timeout_seconds))
        // Disable Nagle's Algorithm since we don't want packets to wait
        .tcp_nodelay(true)
        .tcp_keepalive(Some(Duration::from_secs(config.tcp_keepalive_seconds)))
        .http2_keep_alive_interval(Duration::from_secs(
            config.http2_keepalive_interval_seconds,
        ))
        // Use a fixed timeout for keep-alive pings to keep configuration simple
        // since this is a standalone configuration
        .keep_alive_timeout(Duration::from_secs(20))
        .keep_alive_while_idle(true);
    endpoint.connect().await
}

/// Creates a gRPC client endpoint (without connecting) for customization.
/// This is typically used when TLS or other custom configuration is needed.
/// If `config` is provided, standard timeout and keepalive settings are applied.
pub fn create_grpc_client_endpoint<D>(
    dst: D,
    config: Option<&GrpcClientConfig>,
) -> std::result::Result<Endpoint, Error>
where
    D: std::convert::TryInto<tonic::transport::Endpoint>,
    D::Error: Into<StdError>,
{
    let endpoint = tonic::transport::Endpoint::new(dst)?;
    if let Some(config) = config {
        Ok(endpoint
            .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
            .timeout(Duration::from_secs(config.timeout_seconds))
            .tcp_nodelay(true)
            .tcp_keepalive(Some(Duration::from_secs(config.tcp_keepalive_seconds)))
            .http2_keep_alive_interval(Duration::from_secs(
                config.http2_keepalive_interval_seconds,
            ))
            .keep_alive_timeout(Duration::from_secs(20))
            .keep_alive_while_idle(true))
    } else {
        Ok(endpoint)
    }
}

/// Creates a gRPC server builder with the specified configuration.
pub fn create_grpc_server(config: &GrpcServerConfig) -> Server {
    Server::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        // Disable Nagle's Algorithm since we don't want packets to wait
        .tcp_nodelay(true)
        .tcp_keepalive(Some(Duration::from_secs(config.tcp_keepalive_seconds)))
        .http2_keepalive_interval(Some(Duration::from_secs(
            config.http2_keepalive_interval_seconds,
        )))
        .http2_keepalive_timeout(Some(Duration::from_secs(
            config.http2_keepalive_timeout_seconds,
        )))
}

/// Recursively collects metrics from an execution plan and all its children.
pub fn collect_plan_metrics(plan: &dyn ExecutionPlan) -> Vec<MetricsSet> {
    let mut metrics_array = Vec::<MetricsSet>::new();
    if let Some(metrics) = plan.metrics() {
        metrics_array.push(metrics);
    }
    plan.children().iter().for_each(|c| {
        collect_plan_metrics(c.as_ref())
            .into_iter()
            .for_each(|e| metrics_array.push(e))
    });
    metrics_array
}

/// Given an interval in seconds, get the time in seconds before now
pub fn get_time_before(interval_seconds: u64) -> u64 {
    let now_epoch_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    now_epoch_ts
        .checked_sub(Duration::from_secs(interval_seconds))
        .unwrap_or_else(|| Duration::from_secs(0))
        .as_secs()
}

/// Compacts View arrays ONLY IF the total batch size exceeds the gRPC limit.
pub fn maybe_gc_batch(
    batch: RecordBatch,
    max_grpc_message_size: usize,
) -> Result<RecordBatch> {
    // 1. Check Total Physical Size
    // get_array_memory_size() returns the size of the underlying buffers.
    // For BinaryView, this includes the full size of the referenced buffers.
    let total_size: usize = batch
        .columns()
        .iter()
        .map(|c| c.get_array_memory_size())
        .sum();
    // 2. Fast Path: If we are within limits, do nothing.
    if total_size <= max_grpc_message_size {
        return Ok(batch.clone());
    }

    // 3. Slow Path: We are exceeding the limit. Try to compact View columns.
    let mut new_columns: Vec<ArrayRef> = Vec::with_capacity(batch.num_columns());
    let mut changed = false;

    for col in batch.columns() {
        let new_col = match col.data_type() {
            DataType::Utf8View => {
                let arr = col.as_string_view();
                // We blindly compact if we are over the limit, assuming the View
                // is the cause of the bloat. We could add a ratio check here too,
                // but if we are over the limit, we MUST try to reduce size.
                changed = true;
                Arc::new(arr.gc()) as ArrayRef
            }
            DataType::BinaryView => {
                let arr = col.as_binary_view();
                changed = true;
                Arc::new(arr.gc()) as ArrayRef
            }
            _ => col.clone(),
        };
        new_columns.push(new_col);
    }

    if changed {
        // Map the DataFusion/Arrow error to BallistaError
        RecordBatch::try_new(batch.schema(), new_columns)
            .map_err(DataFusionError::from)
            .map_err(BallistaError::from)
    } else {
        Ok(batch.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::array::StringViewBuilder;
    use datafusion::arrow::datatypes::{DataType, Field, Schema};

    #[test]
    fn test_grpc_client_config_from_ballista_config() {
        let ballista_config = BallistaConfig::default();
        let grpc_config = GrpcClientConfig::from(&ballista_config);

        // Verify the conversion picks up the right values
        assert_eq!(
            grpc_config.connect_timeout_seconds,
            ballista_config.default_grpc_client_connect_timeout_seconds() as u64
        );
        assert_eq!(
            grpc_config.timeout_seconds,
            ballista_config.default_grpc_client_timeout_seconds() as u64
        );
        assert_eq!(
            grpc_config.tcp_keepalive_seconds,
            ballista_config.default_grpc_client_tcp_keepalive_seconds() as u64
        );
        assert_eq!(
            grpc_config.http2_keepalive_interval_seconds,
            ballista_config.default_grpc_client_http2_keepalive_interval_seconds() as u64
        );
    }

    #[test]
    fn test_create_grpc_client_endpoint_with_config() {
        let config = GrpcClientConfig {
            connect_timeout_seconds: 10,
            timeout_seconds: 30,
            tcp_keepalive_seconds: 1800,
            http2_keepalive_interval_seconds: 150,
        };
        let result = create_grpc_client_endpoint("http://localhost:50051", Some(&config));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_grpc_client_endpoint_invalid_url() {
        let result = create_grpc_client_endpoint("not a valid url", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_string_view_compaction() {
        let mut builder = StringViewBuilder::with_capacity(100);

        // 1. Append a string that is long enough to NOT be inlined (> 12 bytes),
        // but small enough to show compaction works (e.g., 20 bytes).
        let small_string = "a".repeat(20);
        builder.append_value(&small_string);

        // 2. Append a HUGE string (1MB) to bloat the buffer.
        // Because they are built together, they share the same underlying buffer.
        let big_string = "b".repeat(1024 * 1024);
        builder.append_value(&big_string);

        let array = builder.finish();

        // 3. Slice the array to keep ONLY the first element (the small string).
        // The array logically has only 20 bytes of data, but the underlying
        // buffer is 1MB + 20 bytes.
        let sliced_array = array.slice(0, 1);

        // 4. Create a batch
        let schema = Arc::new(Schema::new(vec![Field::new(
            "a",
            DataType::Utf8View,
            false,
        )]));
        let batch =
            RecordBatch::try_new(schema, vec![Arc::new(sliced_array) as ArrayRef])
                .unwrap();

        let initial_size: usize = batch
            .columns()
            .iter()
            .map(|c| c.get_array_memory_size())
            .sum();

        // Confirm the buffer is huge (because of the hidden big_string)
        assert!(initial_size >= 1024 * 1024);

        // 5. Force compaction
        // The limit (10KB) is smaller than the current size (1MB), so GC triggers.
        let compacted_batch = maybe_gc_batch(batch, 10 * 1024).unwrap();

        let compacted_size: usize = compacted_batch
            .columns()
            .iter()
            .map(|c| c.get_array_memory_size())
            .sum();

        // 6. The compacted size should now be tiny.
        assert!(compacted_size < 1024); // Should be very small (~100-200 bytes overhead)

        // Verify data correctness
        let res_col = compacted_batch.column(0).as_string_view();
        assert_eq!(res_col.value(0), small_string);
    }
}
