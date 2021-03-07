use actix_web::web::Data as ShareData;

use delay_timer::prelude::*;

use serde::{Deserialize, Serialize};

use anyhow::{anyhow, Error as AnyError, Result as AnyResult};

use rsa::pem;
use rsa::RSAPrivateKey;

use sysinfo::{Process as SysProcess, ProcessExt, System, SystemExt};

use async_lock::RwLock;

use std::collections::HashMap;
use std::convert::{From, TryFrom, TryInto};
use std::env::var_os as get_env_val;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

pub(crate) type SharedDelayTimer = ShareData<DelayTimer>;

#[derive(Debug, Clone)]
pub(crate) struct SecurityKey(pub(crate) RSAPrivateKey);

impl SecurityKey {
    /// Get delicate-executor's security level from env.
    pub(crate) fn get_app_security_key() -> Option<Self> {
        get_env_val("DELICATE_SECURITY_KEY").and_then(|s| {
            fs::read(s)
                .ok()
                .map(|v| SecurityKey(pem::parse(v).unwrap().try_into().unwrap()))
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SecurityConf {
    pub(crate) security_level: SecurityLevel,
    pub(crate) rsa_private_key: Option<SecurityKey>,
}

// TODO:
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct SystemMirror {
    inner_system: RwLock<System>,
    inner_snapshot: RwLock<SystemSnapshot>,
}

impl SystemMirror {
    pub(crate) async fn refresh_all(&self) {
        {
            let mut system = self.inner_system.write().await;
            system.refresh_all();
        }

        {
            let system = self.inner_system.read().await;
            let inner_processes = system.get_processes();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DelicateConf {
    pub(crate) security_conf: SecurityConf,
}

impl Default for SecurityConf {
    fn default() -> Self {
        let security_level = SecurityLevel::get_app_security_level();
        let rsa_private_key = SecurityKey::get_app_security_key();

        assert!(
            matches!(security_level, SecurityLevel::Normal if rsa_private_key.is_some()), "When the security level is Normal, the initialization `delicate-executor` must contain the secret key (DELICATE_SECURITY_KEY)"
        );

        Self {
            security_level: SecurityLevel::get_app_security_level(),
            rsa_private_key: SecurityKey::get_app_security_key(),
        }
    }
}

impl Default for DelicateConf {
    fn default() -> Self {
        DelicateConf {
            security_conf: SecurityConf::default(),
        }
    }
}

/// Delicate's security level.
/// The distinction in security level is reflected at `bind_executor-api`.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub(crate) enum SecurityLevel {
    /// There are no strict restrictions.
    ZeroRestriction,
    /// Normal security validation, encrypted validation is required at `bind_executor-api`.
    Normal,
}

impl Default for SecurityLevel {
    fn default() -> Self {
        SecurityLevel::ZeroRestriction
    }
}

impl TryFrom<u16> for SecurityLevel {
    type Error = AnyError;

    fn try_from(value: u16) -> AnyResult<SecurityLevel> {
        match value {
            0 => Ok(SecurityLevel::ZeroRestriction),
            1 => Ok(SecurityLevel::Normal),
            _ => Err(anyhow!("SecurityLevel missed.")),
        }
    }
}

impl SecurityLevel {
    /// Get delicate-executor's security level from env.
    pub(crate) fn get_app_security_level() -> Self {
        get_env_val("DELICATE_SECURITY_LEVEL").map_or(SecurityLevel::default(), |e| {
            e.to_str()
                .map(|s| u16::from_str(s).ok())
                .flatten()
                .map(|e| e.try_into().ok())
                .flatten()
                .expect("SecurityLevel missed.")
        })
    }
}

/// Uniform public message response format.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UnifiedResponseMessages {
    code: i8,
    msg: String,
}
impl UnifiedResponseMessages {
    pub(crate) fn success() -> Self {
        UnifiedResponseMessages::default()
    }

    pub(crate) fn error() -> Self {
        UnifiedResponseMessages {
            code: -1,
            ..Default::default()
        }
    }

    pub(crate) fn customized_error_msg(mut self, msg: String) -> Self {
        self.msg = msg;

        self
    }

    #[allow(dead_code)]
    pub(crate) fn customized_error_code(mut self, code: i8) -> Self {
        self.code = code;

        self
    }

    #[allow(dead_code)]
    pub(crate) fn reverse(mut self) -> Self {
        self.code = -1 - self.code;
        self
    }
}

impl<T> From<AnyResult<T>> for UnifiedResponseMessages {
    fn from(value: AnyResult<T>) -> Self {
        match value {
            Ok(_) => Self::success(),
            Err(e) => Self::error().customized_error_msg(e.to_string()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SystemSnapshot {
    Processes: Processes,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Processes {
    inner: HashMap<usize, Process>,
}

impl From<&HashMap<usize, SysProcess>> for Processes {
    fn from(value: &HashMap<usize, SysProcess>) -> Processes {
        // let inner: HashMap<usize, Process> = value.iter().map(|(_, s)| s.into()).collect();
        // Processes { inner }
        todo!()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct Process {
    name: String,
    cmd: Vec<String>,
    exe: PathBuf,
    pid: usize,
    memory: u64,
    virtual_memory: u64,
    parent: Option<usize>,
    start_time: u64,
    cpu_usage: f32,
    //TODO: ProcessStatus should be stored in Process;
}

impl From<&SysProcess> for Process {
    fn from(sys_process: &SysProcess) -> Self {
        Process {
            name: sys_process.name().to_string(),
            cmd: sys_process.cmd().to_vec(),
            exe: sys_process.exe().to_path_buf(),
            pid: sys_process.pid(),
            memory: sys_process.memory(),
            virtual_memory: sys_process.virtual_memory(),
            parent: sys_process.parent(),
            start_time: sys_process.start_time(),
            cpu_usage: sys_process.cpu_usage(),
        }
    }
}
