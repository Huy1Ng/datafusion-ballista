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
//

//! Ballista scheduler specific configuration

//
// configure me does not follow clippy spec
// we need to replace it with some other library
// as mentioned in https://github.com/apache/datafusion-ballista/issues/1271
//
#![allow(clippy::uninlined_format_args)]

use crate::cluster::DistributionPolicy;
use crate::SessionBuilder;
use ballista_core::{config::TaskSchedulingPolicy, ConfigProducer};
use datafusion_proto::logical_plan::LogicalExtensionCodec;
use datafusion_proto::physical_plan::PhysicalExtensionCodec;
use std::sync::Arc;

#[cfg(feature = "build-binary")]

include!(concat!(
    env!("OUT_DIR"),
    "/scheduler_configure_me_config.rs"
));

/// Configurations for the ballista scheduler of scheduling jobs and tasks
#[derive(Clone)]
pub struct SchedulerConfig {
    /// Namespace of this scheduler. Schedulers using the same cluster storage and namespace
    /// will share global cluster state.
    pub namespace: String,
    /// The external hostname of the scheduler
    pub external_host: String,
    /// The bind host for the scheduler's gRPC service
    pub bind_host: String,
    /// The bind port for the scheduler's gRPC service
    pub bind_port: u16,
    /// The task scheduling policy for the scheduler
    pub scheduling_policy: TaskSchedulingPolicy,
    /// The event loop buffer size. for a system of high throughput, a larger value like 1000000 is recommended
    pub event_loop_buffer_size: u32,
    /// Policy of distributing tasks to available executor slots. For a cluster with single scheduler, round-robin is recommended
    pub task_distribution: TaskDistributionPolicy,
    /// The delayed interval for cleaning up finished job data, mainly the shuffle data, 0 means the cleaning up is disabled
    pub finished_job_data_clean_up_interval_seconds: u64,
    /// The delayed interval for cleaning up finished job state stored in the backend, 0 means the cleaning up is disabled.
    pub finished_job_state_clean_up_interval_seconds: u64,
    /// The route endpoint for proxying flight sql results via scheduler
    pub advertise_flight_sql_endpoint: Option<String>,
    /// If provided, submitted jobs which do not have tasks scheduled will be resubmitted after `job_resubmit_interval_ms`
    /// milliseconds
    pub job_resubmit_interval_ms: Option<u64>,
    /// Time in seconds to allow executor for graceful shutdown. Once an executor signals it has entered Terminating status
    /// the scheduler should only consider the executor dead after this time interval has elapsed
    pub executor_termination_grace_period: u64,
    /// The maximum expected processing time of a scheduler event (microseconds). Zero means disable.
    pub scheduler_event_expected_processing_duration: u64,
    /// The maximum size of a decoded message at the grpc server side.
    pub grpc_server_max_decoding_message_size: u32,
    /// The maximum size of an encoded message at the grpc server side.
    pub grpc_server_max_encoding_message_size: u32,
    /// The executor timeout in seconds. It should be longer than executor's heartbeat intervals.
    pub executor_timeout_seconds: u64,
    /// The interval to check expired or dead executors
    pub expire_dead_executor_interval_seconds: u64,

    /// [ConfigProducer] override option
    pub override_config_producer: Option<ConfigProducer>,
    /// [SessionBuilder] override option
    pub override_session_builder: Option<SessionBuilder>,
    /// [PhysicalExtensionCodec] override option
    pub override_logical_codec: Option<Arc<dyn LogicalExtensionCodec>>,
    /// [PhysicalExtensionCodec] override option
    pub override_physical_codec: Option<Arc<dyn PhysicalExtensionCodec>>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            namespace: String::default(),
            external_host: "localhost".into(),
            bind_port: 50050,
            bind_host: "127.0.0.1".into(),
            scheduling_policy: Default::default(),
            event_loop_buffer_size: 10000,
            task_distribution: Default::default(),
            finished_job_data_clean_up_interval_seconds: 300,
            finished_job_state_clean_up_interval_seconds: 3600,
            advertise_flight_sql_endpoint: None,
            job_resubmit_interval_ms: None,
            executor_termination_grace_period: 0,
            scheduler_event_expected_processing_duration: 0,
            grpc_server_max_decoding_message_size: 16777216,
            grpc_server_max_encoding_message_size: 16777216,
            executor_timeout_seconds: 180,
            expire_dead_executor_interval_seconds: 15,
            override_config_producer: None,
            override_session_builder: None,
            override_logical_codec: None,
            override_physical_codec: None,
        }
    }
}

impl SchedulerConfig {
    pub fn scheduler_name(&self) -> String {
        format!("{}:{}", self.external_host, self.bind_port)
    }

    pub fn is_push_staged_scheduling(&self) -> bool {
        matches!(self.scheduling_policy, TaskSchedulingPolicy::PushStaged)
    }

    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.external_host = hostname.into();
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.bind_port = port;
        self
    }

    pub fn with_scheduler_policy(mut self, policy: TaskSchedulingPolicy) -> Self {
        self.scheduling_policy = policy;
        self
    }

    pub fn with_event_loop_buffer_size(mut self, buffer_size: u32) -> Self {
        self.event_loop_buffer_size = buffer_size;
        self
    }

    pub fn with_finished_job_data_clean_up_interval_seconds(
        mut self,
        interval_seconds: u64,
    ) -> Self {
        self.finished_job_data_clean_up_interval_seconds = interval_seconds;
        self
    }

    pub fn with_finished_job_state_clean_up_interval_seconds(
        mut self,
        interval_seconds: u64,
    ) -> Self {
        self.finished_job_state_clean_up_interval_seconds = interval_seconds;
        self
    }

    pub fn with_advertise_flight_sql_endpoint(
        mut self,
        endpoint: Option<String>,
    ) -> Self {
        self.advertise_flight_sql_endpoint = endpoint;
        self
    }

    pub fn with_task_distribution(mut self, policy: TaskDistributionPolicy) -> Self {
        self.task_distribution = policy;
        self
    }

    pub fn with_job_resubmit_interval_ms(mut self, interval_ms: u64) -> Self {
        self.job_resubmit_interval_ms = Some(interval_ms);
        self
    }

    pub fn with_remove_executor_wait_secs(mut self, value: u64) -> Self {
        self.executor_termination_grace_period = value;
        self
    }

    pub fn with_grpc_server_max_decoding_message_size(mut self, value: u32) -> Self {
        self.grpc_server_max_decoding_message_size = value;
        self
    }

    pub fn with_grpc_server_max_encoding_message_size(mut self, value: u32) -> Self {
        self.grpc_server_max_encoding_message_size = value;
        self
    }

    pub fn with_override_config_producer(
        mut self,
        override_config_producer: ConfigProducer,
    ) -> Self {
        self.override_config_producer = Some(override_config_producer);
        self
    }

    pub fn with_override_session_builder(
        mut self,
        override_session_builder: SessionBuilder,
    ) -> Self {
        self.override_session_builder = Some(override_session_builder);
        self
    }
}

/// Policy of distributing tasks to available executor slots
///
/// It needs to be visible to code generated by configure_me
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[cfg_attr(feature = "build-binary", derive(clap::ValueEnum))]
pub enum TaskDistribution {
    /// Eagerly assign tasks to executor slots. This will assign as many task slots per executor
    /// as are currently available
    Bias,
    /// Distribute tasks evenly across executors. This will try and iterate through available executors
    /// and assign one task to each executor until all tasks are assigned.
    RoundRobin,
    /// 1. Firstly, try to bind tasks without scanning source files by [`RoundRobin`] policy.
    /// 2. Then for a task for scanning source files, firstly calculate a hash value based on input files.
    ///    And then bind it with an execute according to consistent hashing policy.
    /// 3. If needed, work stealing can be enabled based on the tolerance of the consistent hashing.
    ConsistentHash,
}

#[cfg(feature = "build-binary")]
impl std::str::FromStr for TaskDistribution {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        clap::ValueEnum::from_str(s, true)
    }
}

#[cfg(feature = "build-binary")]
impl configure_me::parse_arg::ParseArgFromStr for TaskDistribution {
    fn describe_type<W: std::fmt::Write>(mut writer: W) -> std::fmt::Result {
        write!(writer, "The executor slots policy for the scheduler")
    }
}

#[derive(Debug, Clone, Default)]
pub enum TaskDistributionPolicy {
    /// Eagerly assign tasks to executor slots. This will assign as many task slots per executor
    /// as are currently available
    #[default]
    Bias,
    /// Distribute tasks evenly across executors. This will try and iterate through available executors
    /// and assign one task to each executor until all tasks are assigned.
    RoundRobin,
    /// 1. Firstly, try to bind tasks without scanning source files by [`RoundRobin`] policy.
    /// 2. Then for a task for scanning source files, firstly calculate a hash value based on input files.
    ///    And then bind it with an execute according to consistent hashing policy.
    /// 3. If needed, work stealing can be enabled based on the tolerance of the consistent hashing.
    ConsistentHash {
        num_replicas: usize,
        tolerance: usize,
    },
    /// User provided task distribution policy
    Custom(Arc<dyn DistributionPolicy>),
}

#[cfg(feature = "build-binary")]
impl TryFrom<Config> for SchedulerConfig {
    type Error = ballista_core::error::BallistaError;

    fn try_from(opt: Config) -> Result<Self, Self::Error> {
        let task_distribution = match opt.task_distribution {
            TaskDistribution::Bias => TaskDistributionPolicy::Bias,
            TaskDistribution::RoundRobin => TaskDistributionPolicy::RoundRobin,
            TaskDistribution::ConsistentHash => {
                let num_replicas = opt.consistent_hash_num_replicas as usize;
                let tolerance = opt.consistent_hash_tolerance as usize;
                TaskDistributionPolicy::ConsistentHash {
                    num_replicas,
                    tolerance,
                }
            }
        };

        let config = SchedulerConfig {
            namespace: opt.namespace,
            external_host: opt.external_host,
            bind_port: opt.bind_port,
            bind_host: opt.bind_host,
            scheduling_policy: opt.scheduler_policy,
            event_loop_buffer_size: opt.event_loop_buffer_size,
            task_distribution,
            finished_job_data_clean_up_interval_seconds: opt
                .finished_job_data_clean_up_interval_seconds,
            finished_job_state_clean_up_interval_seconds: opt
                .finished_job_state_clean_up_interval_seconds,
            advertise_flight_sql_endpoint: opt.advertise_flight_sql_endpoint,
            job_resubmit_interval_ms: (opt.job_resubmit_interval_ms > 0)
                .then_some(opt.job_resubmit_interval_ms),
            executor_termination_grace_period: opt.executor_termination_grace_period,
            scheduler_event_expected_processing_duration: opt
                .scheduler_event_expected_processing_duration,
            grpc_server_max_decoding_message_size: opt
                .grpc_server_max_decoding_message_size,
            grpc_server_max_encoding_message_size: opt
                .grpc_server_max_encoding_message_size,
            executor_timeout_seconds: opt.executor_timeout_seconds,
            expire_dead_executor_interval_seconds: opt
                .expire_dead_executor_interval_seconds,
            override_config_producer: None,
            override_logical_codec: None,
            override_physical_codec: None,
            override_session_builder: None,
        };

        Ok(config)
    }
}
