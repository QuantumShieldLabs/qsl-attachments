use std::collections::{BTreeMap, HashMap};
use std::ffi::CString;
use std::fs;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::{Path as AxumPath, State};
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::http::{HeaderMap, Response, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{info, warn};

const RESUME_TOKEN_HEADER: &str = "X-QATT-Resume-Token";
const FETCH_CAPABILITY_HEADER: &str = "X-QATT-Fetch-Capability";
const LOCATOR_KIND_V1: &str = "service_ref_v1";
const INTEGRITY_ALG_V1: &str = "sha512_merkle_v1";
const DEFAULT_MAX_CIPHERTEXT_BYTES: u64 = 101 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Config {
    pub storage_root: PathBuf,
    pub bind_addr: SocketAddr,
    pub max_ciphertext_bytes: u64,
    pub max_open_sessions: usize,
    pub storage_reserve_bytes: u64,
    pub session_ttl_secs: u64,
    pub short_retention_ttl_secs: u64,
    pub standard_retention_ttl_secs: u64,
    pub extended_retention_ttl_secs: u64,
    pub invalid_secret_attempt_limit: u32,
    pub invalid_range_attempt_limit: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            storage_root: PathBuf::from("./data"),
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 3000)),
            max_ciphertext_bytes: DEFAULT_MAX_CIPHERTEXT_BYTES,
            max_open_sessions: 32,
            storage_reserve_bytes: 64 * 1024 * 1024,
            session_ttl_secs: 3600,
            short_retention_ttl_secs: 3600,
            standard_retention_ttl_secs: 86_400,
            extended_retention_ttl_secs: 604_800,
            invalid_secret_attempt_limit: 8,
            invalid_range_attempt_limit: 4,
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let mut cfg = Self::default();
        if let Ok(value) = std::env::var("QATT_STORAGE_ROOT") {
            cfg.storage_root = PathBuf::from(value);
        }
        if let Ok(value) = std::env::var("QATT_BIND_ADDR") {
            cfg.bind_addr = value
                .parse()
                .map_err(|e| format!("invalid QATT_BIND_ADDR: {e}"))?;
        }
        parse_env_u64("QATT_MAX_CIPHERTEXT_BYTES", &mut cfg.max_ciphertext_bytes)?;
        parse_env_usize("QATT_MAX_OPEN_SESSIONS", &mut cfg.max_open_sessions)?;
        parse_env_u64("QATT_STORAGE_RESERVE_BYTES", &mut cfg.storage_reserve_bytes)?;
        parse_env_u64("QATT_SESSION_TTL_SECS", &mut cfg.session_ttl_secs)?;
        parse_env_u64(
            "QATT_RETENTION_SHORT_SECS",
            &mut cfg.short_retention_ttl_secs,
        )?;
        parse_env_u64(
            "QATT_RETENTION_STANDARD_SECS",
            &mut cfg.standard_retention_ttl_secs,
        )?;
        parse_env_u64(
            "QATT_RETENTION_EXTENDED_SECS",
            &mut cfg.extended_retention_ttl_secs,
        )?;
        parse_env_u32(
            "QATT_INVALID_SECRET_ATTEMPTS",
            &mut cfg.invalid_secret_attempt_limit,
        )?;
        parse_env_u32(
            "QATT_INVALID_RANGE_ATTEMPTS",
            &mut cfg.invalid_range_attempt_limit,
        )?;
        Ok(cfg)
    }

    fn retention_ttl_secs(&self, retention_class: RetentionClass) -> u64 {
        match retention_class {
            RetentionClass::Short => self.short_retention_ttl_secs,
            RetentionClass::Standard => self.standard_retention_ttl_secs,
            RetentionClass::Extended => self.extended_retention_ttl_secs,
        }
    }
}

fn parse_env_u64(name: &str, target: &mut u64) -> Result<(), String> {
    if let Ok(value) = std::env::var(name) {
        *target = value.parse().map_err(|e| format!("invalid {name}: {e}"))?;
    }
    Ok(())
}

fn parse_env_u32(name: &str, target: &mut u32) -> Result<(), String> {
    if let Ok(value) = std::env::var(name) {
        *target = value.parse().map_err(|e| format!("invalid {name}: {e}"))?;
    }
    Ok(())
}

fn parse_env_usize(name: &str, target: &mut usize) -> Result<(), String> {
    if let Ok(value) = std::env::var(name) {
        *target = value.parse().map_err(|e| format!("invalid {name}: {e}"))?;
    }
    Ok(())
}

pub trait Clock: Send + Sync {
    fn now_unix_s(&self) -> u64;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_s(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[derive(Clone, Debug)]
pub struct TestClock {
    now_unix_s: Arc<Mutex<u64>>,
}

impl TestClock {
    pub fn new(initial: u64) -> Self {
        Self {
            now_unix_s: Arc::new(Mutex::new(initial)),
        }
    }

    pub fn advance(&self, delta: u64) {
        let mut guard = self.now_unix_s.lock().expect("clock lock");
        *guard += delta;
    }
}

impl Clock for TestClock {
    fn now_unix_s(&self) -> u64 {
        *self.now_unix_s.lock().expect("clock lock")
    }
}

pub trait DiskSpace: Send + Sync {
    fn available_bytes(&self, path: &Path) -> io::Result<u64>;
}

#[derive(Debug, Default)]
pub struct SystemDiskSpace;

impl DiskSpace for SystemDiskSpace {
    fn available_bytes(&self, path: &Path) -> io::Result<u64> {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains nul"))?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        let rc = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        let stats = unsafe { stats.assume_init() };
        Ok(stats.f_bavail.saturating_mul(stats.f_frsize))
    }
}

#[derive(Clone, Debug)]
pub struct TestDiskSpace {
    available_bytes: Arc<Mutex<u64>>,
}

impl TestDiskSpace {
    pub fn new(initial: u64) -> Self {
        Self {
            available_bytes: Arc::new(Mutex::new(initial)),
        }
    }

    pub fn set_available_bytes(&self, value: u64) {
        *self.available_bytes.lock().expect("disk lock") = value;
    }
}

impl DiskSpace for TestDiskSpace {
    fn available_bytes(&self, _path: &Path) -> io::Result<u64> {
        Ok(*self.available_bytes.lock().expect("disk lock"))
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AuditEvent {
    pub kind: String,
    pub session_handle: Option<String>,
    pub locator_handle: Option<String>,
    pub attachment_handle: Option<String>,
    pub reason_code: Option<String>,
}

#[derive(Clone, Default)]
pub struct AuditLog {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl AuditLog {
    fn record(&self, event: AuditEvent) {
        info!(
            kind = %event.kind,
            session_handle = event.session_handle.as_deref().unwrap_or(""),
            locator_handle = event.locator_handle.as_deref().unwrap_or(""),
            attachment_handle = event.attachment_handle.as_deref().unwrap_or(""),
            reason_code = event.reason_code.as_deref().unwrap_or(""),
            "qatt audit event"
        );
        self.events.lock().expect("audit lock").push(event);
    }

    pub fn snapshot(&self) -> Vec<AuditEvent> {
        self.events.lock().expect("audit lock").clone()
    }
}

#[derive(Default)]
struct AbuseTracker {
    invalid_resume_attempts: HashMap<String, u32>,
    invalid_fetch_attempts: HashMap<String, u32>,
    invalid_range_attempts: HashMap<String, u32>,
}

impl AbuseTracker {
    fn register_resume_failure(&mut self, session_id: &str, limit: u32) -> bool {
        register_attempt(&mut self.invalid_resume_attempts, session_id, limit)
    }

    fn clear_resume_failures(&mut self, session_id: &str) {
        self.invalid_resume_attempts.remove(session_id);
    }

    fn register_fetch_failure(&mut self, locator_ref: &str, limit: u32) -> bool {
        register_attempt(&mut self.invalid_fetch_attempts, locator_ref, limit)
    }

    fn clear_fetch_failures(&mut self, locator_ref: &str) {
        self.invalid_fetch_attempts.remove(locator_ref);
    }

    fn register_range_failure(&mut self, locator_ref: &str, limit: u32) -> bool {
        register_attempt(&mut self.invalid_range_attempts, locator_ref, limit)
    }

    fn clear_range_failures(&mut self, locator_ref: &str) {
        self.invalid_range_attempts.remove(locator_ref);
    }
}

fn register_attempt(map: &mut HashMap<String, u32>, key: &str, limit: u32) -> bool {
    let entry = map.entry(key.to_owned()).or_insert(0);
    *entry += 1;
    *entry > limit
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<InnerState>,
}

struct InnerState {
    config: Config,
    clock: Arc<dyn Clock>,
    disk_space: Arc<dyn DiskSpace>,
    storage: Storage,
    audit: AuditLog,
    mutation_lock: AsyncMutex<()>,
    abuse_tracker: AsyncMutex<AbuseTracker>,
}

impl AppState {
    pub fn new(config: Config, clock: Arc<dyn Clock>) -> io::Result<Self> {
        Self::new_with_disk_space(config, clock, Arc::new(SystemDiskSpace))
    }

    pub fn new_with_disk_space(
        config: Config,
        clock: Arc<dyn Clock>,
        disk_space: Arc<dyn DiskSpace>,
    ) -> io::Result<Self> {
        let storage = Storage::new(config.storage_root.clone());
        storage.ensure_layout()?;
        Ok(Self {
            inner: Arc::new(InnerState {
                config,
                clock,
                disk_space,
                storage,
                audit: AuditLog::default(),
                mutation_lock: AsyncMutex::new(()),
                abuse_tracker: AsyncMutex::new(AbuseTracker::default()),
            }),
        })
    }

    pub fn audit_snapshot(&self) -> Vec<AuditEvent> {
        self.inner.audit.snapshot()
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    fn ensure_disk_headroom(
        &self,
        additional_bytes: u64,
        session_id: Option<&str>,
        attachment_id: Option<&str>,
    ) -> Result<(), ServiceError> {
        let required_bytes =
            additional_bytes.saturating_add(self.inner.config.storage_reserve_bytes);
        let available_bytes = self
            .inner
            .disk_space
            .available_bytes(&self.inner.config.storage_root)?;
        if available_bytes >= required_bytes {
            return Ok(());
        }
        self.inner.audit.record(AuditEvent {
            kind: "quota_reject".to_owned(),
            session_handle: audit_handle_opt("session", session_id),
            locator_handle: None,
            attachment_handle: audit_handle_opt("attachment", attachment_id),
            reason_code: Some("REJECT_QATTSVC_QUOTA".to_owned()),
        });
        Err(ServiceError::quota(
            "insufficient free disk for staged and committed attachment data",
        ))
    }

    async fn sweep_expired(&self, now: u64) -> Result<(), ServiceError> {
        let sessions = self.inner.storage.load_all_sessions()?;
        for mut session in sessions {
            if matches!(
                session.state,
                SessionState::Created | SessionState::Uploading | SessionState::Committable
            ) && now > session.session_expires_at_unix_s
            {
                session.state = SessionState::ExpiredSession;
                session.resume_token_hash = None;
                self.inner
                    .storage
                    .clear_session_parts(&session.session_id)?;
                self.inner.storage.save_session(&session)?;
                self.inner.audit.record(AuditEvent {
                    kind: "session_expired".to_owned(),
                    session_handle: audit_handle_opt("session", Some(session.session_id.as_str())),
                    locator_handle: None,
                    attachment_handle: audit_handle_opt(
                        "attachment",
                        Some(session.attachment_id.as_str()),
                    ),
                    reason_code: Some("REJECT_QATTSVC_EXPIRED".to_owned()),
                });
            }
        }

        let objects = self.inner.storage.load_all_objects()?;
        for mut object in objects {
            if object.object_state == ObjectState::CommittedObject && now > object.expires_at_unix_s
            {
                object.object_state = ObjectState::ExpiredObject;
                object.fetch_capability_hash = None;
                self.inner
                    .storage
                    .remove_object_bytes(&object.locator_ref)?;
                self.inner.storage.save_object(&object)?;
                self.inner.audit.record(AuditEvent {
                    kind: "object_expired".to_owned(),
                    session_handle: None,
                    locator_handle: audit_handle_opt("locator", Some(object.locator_ref.as_str())),
                    attachment_handle: audit_handle_opt(
                        "attachment",
                        Some(object.attachment_id.as_str()),
                    ),
                    reason_code: Some("REJECT_QATTSVC_EXPIRED".to_owned()),
                });
            }
        }

        Ok(())
    }

    async fn create_session(
        &self,
        uri: &Uri,
        request: CreateSessionRequest,
    ) -> Result<CreateSessionResponse, ServiceError> {
        reject_noncanonical_query(uri)?;
        validate_create_session_request(&request, &self.inner.config)?;
        let now = self.inner.clock.now_unix_s();
        let _guard = self.inner.mutation_lock.lock().await;
        self.sweep_expired(now).await?;

        let open_sessions = self
            .inner
            .storage
            .load_all_sessions()?
            .into_iter()
            .filter(|session| {
                matches!(
                    session.state,
                    SessionState::Created | SessionState::Uploading | SessionState::Committable
                )
            })
            .count();
        if open_sessions >= self.inner.config.max_open_sessions {
            return Err(ServiceError::quota("too many open sessions"));
        }
        if self
            .inner
            .storage
            .has_active_attachment(&request.attachment_id, now)?
        {
            return Err(ServiceError::policy("attachment_id already active"));
        }
        self.ensure_disk_headroom(
            request.ciphertext_len.saturating_mul(2),
            None,
            Some(&request.attachment_id),
        )?;

        let session_id = random_token(18);
        let resume_token = random_token(32);
        let session = SessionMeta {
            session_id: session_id.clone(),
            attachment_id: request.attachment_id,
            ciphertext_len: request.ciphertext_len,
            part_size_class: request.part_size_class,
            part_count: request.part_count,
            integrity_alg: request.integrity_alg,
            integrity_root: request.integrity_root,
            retention_class: request.retention_class,
            session_expires_at_unix_s: now + self.inner.config.session_ttl_secs,
            state: SessionState::Created,
            resume_token_hash: Some(hash_secret(&resume_token)),
            stored_parts: BTreeMap::new(),
        };
        self.inner.storage.create_session(&session)?;
        self.inner.audit.record(AuditEvent {
            kind: "session_created".to_owned(),
            session_handle: audit_handle_opt("session", Some(session_id.as_str())),
            locator_handle: None,
            attachment_handle: audit_handle_opt("attachment", Some(session.attachment_id.as_str())),
            reason_code: None,
        });

        Ok(CreateSessionResponse {
            session_id,
            resume_token,
            session_state: SessionState::Created,
            ciphertext_len: session.ciphertext_len,
            part_size_class: session.part_size_class,
            part_count: session.part_count,
            retention_class: session.retention_class,
            session_expires_at_unix_s: session.session_expires_at_unix_s,
        })
    }

    async fn upload_part(
        &self,
        uri: &Uri,
        session_id: &str,
        part_index_raw: &str,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Result<UploadPartResponse, ServiceError> {
        reject_noncanonical_query(uri)?;
        validate_non_secret_ref(session_id).map_err(ServiceError::secret_url_placement)?;
        let part_index: u32 = part_index_raw
            .parse()
            .map_err(|_| ServiceError::part_index("invalid part index"))?;
        let resume_token = extract_required_secret(headers, RESUME_TOKEN_HEADER)?;

        let now = self.inner.clock.now_unix_s();
        let _guard = self.inner.mutation_lock.lock().await;
        self.sweep_expired(now).await?;

        let mut session = self
            .inner
            .storage
            .load_session(session_id)?
            .ok_or_else(|| ServiceError::session_state("unknown session"))?;
        if session.state == SessionState::ExpiredSession {
            return Err(ServiceError::expired("session is no longer active"));
        }

        validate_resume_token(&session, session_id, &resume_token, self).await?;

        if matches!(
            session.state,
            SessionState::AbortedSession | SessionState::ExpiredSession
        ) {
            return Err(ServiceError::session_state("session is not open"));
        }

        if part_index >= session.part_count {
            return Err(ServiceError::part_index("part index out of bounds"));
        }

        let expected_len = expected_part_length(&session, part_index);
        if body.len() as u64 != expected_len {
            return Err(ServiceError::part_length(
                "part length does not match declared shape",
            ));
        }
        self.ensure_disk_headroom(
            expected_len,
            Some(&session.session_id),
            Some(&session.attachment_id),
        )?;

        if let Some(existing) = self
            .inner
            .storage
            .read_part(&session.session_id, part_index)?
        {
            if existing == body {
                let missing_part_ranges = missing_part_ranges(&session);
                return Ok(UploadPartResponse {
                    session_id: session.session_id,
                    session_state: session.state,
                    received_part_index: part_index,
                    stored_part_count: session.stored_parts.len() as u32,
                    missing_part_ranges,
                });
            }
            return Err(ServiceError::part_replay_mismatch(
                "part replay bytes differ from staged bytes",
            ));
        }

        self.inner
            .storage
            .write_part(&session.session_id, part_index, &body)?;
        session.stored_parts.insert(part_index, expected_len);
        session.state = if session.stored_parts.len() as u32 == session.part_count {
            SessionState::Committable
        } else {
            SessionState::Uploading
        };
        self.inner.storage.save_session(&session)?;
        self.inner.audit.record(AuditEvent {
            kind: "part_uploaded".to_owned(),
            session_handle: audit_handle_opt("session", Some(session.session_id.as_str())),
            locator_handle: None,
            attachment_handle: audit_handle_opt("attachment", Some(session.attachment_id.as_str())),
            reason_code: None,
        });

        Ok(UploadPartResponse {
            session_id: session.session_id.clone(),
            session_state: session.state,
            received_part_index: part_index,
            stored_part_count: session.stored_parts.len() as u32,
            missing_part_ranges: missing_part_ranges(&session),
        })
    }

    async fn session_status(
        &self,
        uri: &Uri,
        session_id: &str,
        headers: &HeaderMap,
    ) -> Result<SessionStatusResponse, ServiceError> {
        reject_noncanonical_query(uri)?;
        validate_non_secret_ref(session_id).map_err(ServiceError::secret_url_placement)?;
        let resume_token = extract_required_secret(headers, RESUME_TOKEN_HEADER)?;
        let now = self.inner.clock.now_unix_s();
        let _guard = self.inner.mutation_lock.lock().await;
        self.sweep_expired(now).await?;

        let session = self
            .inner
            .storage
            .load_session(session_id)?
            .ok_or_else(|| ServiceError::session_state("unknown session"))?;
        if session.state == SessionState::ExpiredSession {
            return Err(ServiceError::expired("session is no longer active"));
        }
        validate_resume_token(&session, session_id, &resume_token, self).await?;
        if matches!(
            session.state,
            SessionState::AbortedSession | SessionState::ExpiredSession
        ) {
            return Err(ServiceError::expired("session is no longer active"));
        }

        let missing_part_ranges = missing_part_ranges(&session);

        Ok(SessionStatusResponse {
            session_id: session.session_id,
            session_state: session.state,
            attachment_id: session.attachment_id,
            ciphertext_len: session.ciphertext_len,
            part_size_class: session.part_size_class,
            part_count: session.part_count,
            stored_part_count: session.stored_parts.len() as u32,
            missing_part_ranges,
            retention_class: session.retention_class,
            session_expires_at_unix_s: session.session_expires_at_unix_s,
        })
    }

    async fn commit_session(
        &self,
        uri: &Uri,
        session_id: &str,
        headers: &HeaderMap,
        request: CommitRequest,
    ) -> Result<CommitResponse, ServiceError> {
        reject_noncanonical_query(uri)?;
        validate_non_secret_ref(session_id).map_err(ServiceError::secret_url_placement)?;
        let resume_token = extract_required_secret(headers, RESUME_TOKEN_HEADER)?;
        let now = self.inner.clock.now_unix_s();
        let _guard = self.inner.mutation_lock.lock().await;
        self.sweep_expired(now).await?;

        let session = self
            .inner
            .storage
            .load_session(session_id)?
            .ok_or_else(|| ServiceError::session_state("unknown session"))?;
        if session.state == SessionState::ExpiredSession {
            return Err(ServiceError::expired("session is no longer active"));
        }
        validate_resume_token(&session, session_id, &resume_token, self).await?;
        if matches!(
            session.state,
            SessionState::AbortedSession | SessionState::ExpiredSession
        ) {
            return Err(ServiceError::expired("session is no longer active"));
        }
        if session.state != SessionState::Committable {
            return Err(ServiceError::commit_incomplete(
                "session is not committable",
            ));
        }
        validate_commit_request(&request, &session)?;

        let parts = self.inner.storage.read_all_parts(&session)?;
        if parts.len() as u32 != session.part_count {
            return Err(ServiceError::commit_incomplete(
                "required parts are missing",
            ));
        }
        for (idx, bytes) in &parts {
            let expected = expected_part_length(&session, *idx);
            if bytes.len() as u64 != expected {
                return Err(ServiceError::part_length("stored part length is invalid"));
            }
        }
        let ordered_parts: Vec<Vec<u8>> = (0..session.part_count)
            .map(|index| {
                parts
                    .iter()
                    .find(|(candidate, _)| *candidate == index)
                    .map(|(_, bytes)| bytes.clone())
                    .ok_or_else(|| ServiceError::commit_incomplete("required parts are missing"))
            })
            .collect::<Result<_, _>>()?;
        let computed_root = sha512_merkle_root(&ordered_parts);
        if computed_root != session.integrity_root {
            return Err(ServiceError::commit_mismatch("integrity root mismatch"));
        }
        self.ensure_disk_headroom(
            session.ciphertext_len,
            Some(&session.session_id),
            Some(&session.attachment_id),
        )?;

        let locator_ref = random_token(18);
        let fetch_capability = random_token(32);
        let expires_at_unix_s = now
            + self
                .inner
                .config
                .retention_ttl_secs(session.retention_class);
        let object = ObjectMeta {
            attachment_id: session.attachment_id.clone(),
            locator_kind: LOCATOR_KIND_V1.to_owned(),
            locator_ref: locator_ref.clone(),
            fetch_capability_hash: Some(hash_secret(&fetch_capability)),
            ciphertext_len: session.ciphertext_len,
            part_size_class: session.part_size_class,
            part_count: session.part_count,
            integrity_alg: session.integrity_alg.clone(),
            integrity_root: session.integrity_root.clone(),
            retention_class: session.retention_class,
            expires_at_unix_s,
            object_state: ObjectState::CommittedObject,
        };
        self.inner.storage.create_object(&object, &ordered_parts)?;
        self.inner.storage.remove_session(&session.session_id)?;
        self.inner.audit.record(AuditEvent {
            kind: "session_committed".to_owned(),
            session_handle: audit_handle_opt("session", Some(session.session_id.as_str())),
            locator_handle: audit_handle_opt("locator", Some(locator_ref.as_str())),
            attachment_handle: audit_handle_opt("attachment", Some(session.attachment_id.as_str())),
            reason_code: None,
        });

        Ok(CommitResponse {
            attachment_id: object.attachment_id,
            locator_kind: object.locator_kind,
            locator_ref,
            fetch_capability,
            ciphertext_len: object.ciphertext_len,
            part_size_class: object.part_size_class,
            part_count: object.part_count,
            integrity_alg: object.integrity_alg,
            integrity_root: object.integrity_root,
            retention_class: object.retention_class,
            expires_at_unix_s,
            object_state: ObjectState::CommittedObject,
        })
    }

    async fn abort_session(
        &self,
        uri: &Uri,
        session_id: &str,
        headers: &HeaderMap,
    ) -> Result<AbortResponse, ServiceError> {
        reject_noncanonical_query(uri)?;
        validate_non_secret_ref(session_id).map_err(ServiceError::secret_url_placement)?;
        let resume_token = extract_required_secret(headers, RESUME_TOKEN_HEADER)?;
        let now = self.inner.clock.now_unix_s();
        let _guard = self.inner.mutation_lock.lock().await;
        self.sweep_expired(now).await?;

        let mut session = self
            .inner
            .storage
            .load_session(session_id)?
            .ok_or_else(|| ServiceError::session_state("unknown session"))?;
        if session.state == SessionState::ExpiredSession {
            return Err(ServiceError::expired("session is no longer active"));
        }
        validate_resume_token(&session, session_id, &resume_token, self).await?;
        if matches!(
            session.state,
            SessionState::AbortedSession | SessionState::ExpiredSession
        ) {
            return Err(ServiceError::session_state("session is not open"));
        }
        session.state = SessionState::AbortedSession;
        session.resume_token_hash = None;
        self.inner
            .storage
            .clear_session_parts(&session.session_id)?;
        self.inner.storage.save_session(&session)?;
        self.inner.audit.record(AuditEvent {
            kind: "session_aborted".to_owned(),
            session_handle: audit_handle_opt("session", Some(session.session_id.as_str())),
            locator_handle: None,
            attachment_handle: audit_handle_opt("attachment", Some(session.attachment_id.as_str())),
            reason_code: None,
        });
        Ok(AbortResponse {
            session_id: session.session_id,
            session_state: SessionState::AbortedSession,
        })
    }

    async fn fetch_object(
        &self,
        uri: &Uri,
        locator_ref: &str,
        headers: &HeaderMap,
    ) -> Result<Response<Body>, ServiceError> {
        reject_noncanonical_query(uri)?;
        validate_non_secret_ref(locator_ref).map_err(ServiceError::secret_url_placement)?;
        let fetch_capability = extract_required_secret(headers, FETCH_CAPABILITY_HEADER)?;
        let range_header = headers
            .get(RANGE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let now = self.inner.clock.now_unix_s();
        let _guard = self.inner.mutation_lock.lock().await;
        self.sweep_expired(now).await?;

        let object = self
            .inner
            .storage
            .load_object(locator_ref)?
            .ok_or_else(|| ServiceError::locator_unknown("unknown locator_ref"))?;
        if object.object_state == ObjectState::ExpiredObject {
            return Err(ServiceError::expired("object has expired"));
        }
        validate_fetch_capability(&object, locator_ref, &fetch_capability, self).await?;

        let bytes = self.inner.storage.read_object_bytes(locator_ref)?;
        let response = if let Some(range) = range_header {
            let parsed = {
                let mut abuse = self.inner.abuse_tracker.lock().await;
                match parse_single_range(&range, bytes.len() as u64) {
                    Ok(parsed) => {
                        abuse.clear_range_failures(locator_ref);
                        parsed
                    }
                    Err(error) => {
                        if abuse.register_range_failure(
                            locator_ref,
                            self.inner.config.invalid_range_attempt_limit,
                        ) {
                            return Err(ServiceError::abuse("range abuse limit exceeded"));
                        }
                        return Err(error);
                    }
                }
            };
            let body_bytes = bytes[parsed.start as usize..=parsed.end as usize].to_vec();
            let content_range = format!("bytes {}-{}/{}", parsed.start, parsed.end, bytes.len());
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(CONTENT_TYPE, "application/octet-stream")
                .header(ACCEPT_RANGES, "bytes")
                .header(CONTENT_LENGTH, body_bytes.len().to_string())
                .header(CONTENT_RANGE, content_range)
                .body(Body::from(body_bytes))
                .map_err(|_| ServiceError::internal("failed to build range response"))?
        } else {
            let len = bytes.len();
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/octet-stream")
                .header(ACCEPT_RANGES, "bytes")
                .header(CONTENT_LENGTH, len.to_string())
                .body(Body::from(bytes))
                .map_err(|_| ServiceError::internal("failed to build response"))?
        };

        self.inner.audit.record(AuditEvent {
            kind: "object_fetched".to_owned(),
            session_handle: None,
            locator_handle: audit_handle_opt("locator", Some(locator_ref)),
            attachment_handle: audit_handle_opt("attachment", Some(object.attachment_id.as_str())),
            reason_code: None,
        });

        Ok(response)
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/attachments/sessions", post(create_session_handler))
        .route(
            "/v1/attachments/sessions/:session_id/parts/:part_index",
            put(upload_part_handler),
        )
        .route(
            "/v1/attachments/sessions/:session_id",
            get(session_status_handler).delete(abort_session_handler),
        )
        .route(
            "/v1/attachments/sessions/:session_id/commit",
            post(commit_session_handler),
        )
        .route(
            "/v1/attachments/objects/:locator_ref",
            get(fetch_object_handler),
        )
        .with_state(state)
}

async fn create_session_handler(
    State(state): State<AppState>,
    uri: Uri,
    Json(request): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let response = state.create_session(&uri, request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn upload_part_handler(
    State(state): State<AppState>,
    AxumPath((session_id, part_index)): AxumPath<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ServiceError> {
    let response = state
        .upload_part(&uri, &session_id, &part_index, &headers, body)
        .await?;
    Ok(Json(response))
}

async fn session_status_handler(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ServiceError> {
    let response = state.session_status(&uri, &session_id, &headers).await?;
    Ok(Json(response))
}

async fn commit_session_handler(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    uri: Uri,
    headers: HeaderMap,
    Json(request): Json<CommitRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let response = state
        .commit_session(&uri, &session_id, &headers, request)
        .await?;
    Ok(Json(response))
}

async fn abort_session_handler(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ServiceError> {
    let response = state.abort_session(&uri, &session_id, &headers).await?;
    Ok(Json(response))
}

async fn fetch_object_handler(
    State(state): State<AppState>,
    AxumPath(locator_ref): AxumPath<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response<Body>, ServiceError> {
    state.fetch_object(&uri, &locator_ref, &headers).await
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PartSizeClass {
    #[serde(rename = "p64k")]
    P64k,
    #[serde(rename = "p256k")]
    P256k,
    #[serde(rename = "p1024k")]
    P1024k,
}

impl PartSizeClass {
    pub fn bytes(self) -> u64 {
        match self {
            Self::P64k => 65_536,
            Self::P256k => 262_144,
            Self::P1024k => 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetentionClass {
    #[serde(rename = "short")]
    Short,
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "extended")]
    Extended,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionState {
    #[serde(rename = "created")]
    Created,
    #[serde(rename = "uploading")]
    Uploading,
    #[serde(rename = "committable")]
    Committable,
    #[serde(rename = "aborted_session")]
    AbortedSession,
    #[serde(rename = "expired_session")]
    ExpiredSession,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ObjectState {
    #[serde(rename = "committed_object")]
    CommittedObject,
    #[serde(rename = "expired_object")]
    ExpiredObject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissingRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub attachment_id: String,
    pub ciphertext_len: u64,
    pub part_size_class: PartSizeClass,
    pub part_count: u32,
    pub integrity_alg: String,
    pub integrity_root: String,
    pub retention_class: RetentionClass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub resume_token: String,
    pub session_state: SessionState,
    pub ciphertext_len: u64,
    pub part_size_class: PartSizeClass,
    pub part_count: u32,
    pub retention_class: RetentionClass,
    pub session_expires_at_unix_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadPartResponse {
    pub session_id: String,
    pub session_state: SessionState,
    pub received_part_index: u32,
    pub stored_part_count: u32,
    pub missing_part_ranges: Vec<MissingRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusResponse {
    pub session_id: String,
    pub session_state: SessionState,
    pub attachment_id: String,
    pub ciphertext_len: u64,
    pub part_size_class: PartSizeClass,
    pub part_count: u32,
    pub stored_part_count: u32,
    pub missing_part_ranges: Vec<MissingRange>,
    pub retention_class: RetentionClass,
    pub session_expires_at_unix_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRequest {
    pub attachment_id: String,
    pub ciphertext_len: u64,
    pub part_count: u32,
    pub integrity_alg: String,
    pub integrity_root: String,
    pub retention_class: RetentionClass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitResponse {
    pub attachment_id: String,
    pub locator_kind: String,
    pub locator_ref: String,
    pub fetch_capability: String,
    pub ciphertext_len: u64,
    pub part_size_class: PartSizeClass,
    pub part_count: u32,
    pub integrity_alg: String,
    pub integrity_root: String,
    pub retention_class: RetentionClass,
    pub expires_at_unix_s: u64,
    pub object_state: ObjectState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortResponse {
    pub session_id: String,
    pub session_state: SessionState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMeta {
    session_id: String,
    attachment_id: String,
    ciphertext_len: u64,
    part_size_class: PartSizeClass,
    part_count: u32,
    integrity_alg: String,
    integrity_root: String,
    retention_class: RetentionClass,
    session_expires_at_unix_s: u64,
    state: SessionState,
    resume_token_hash: Option<String>,
    stored_parts: BTreeMap<u32, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ObjectMeta {
    attachment_id: String,
    locator_kind: String,
    locator_ref: String,
    fetch_capability_hash: Option<String>,
    ciphertext_len: u64,
    part_size_class: PartSizeClass,
    part_count: u32,
    integrity_alg: String,
    integrity_root: String,
    retention_class: RetentionClass,
    expires_at_unix_s: u64,
    object_state: ObjectState,
}

#[derive(Debug)]
pub struct ServiceError {
    status: StatusCode,
    reason_code: &'static str,
    message: String,
}

impl ServiceError {
    fn new(status: StatusCode, reason_code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            reason_code,
            message: message.into(),
        }
    }

    fn secret_url_placement(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "REJECT_QATTSVC_SECRET_URL_PLACEMENT",
            message,
        )
    }

    fn session_shape(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "REJECT_QATTSVC_SESSION_SHAPE",
            message,
        )
    }

    fn session_state(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "REJECT_QATTSVC_SESSION_STATE",
            message,
        )
    }

    fn resume_token(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "REJECT_QATTSVC_RESUME_TOKEN",
            message,
        )
    }

    fn part_index(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "REJECT_QATTSVC_PART_INDEX",
            message,
        )
    }

    fn part_length(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "REJECT_QATTSVC_PART_LENGTH",
            message,
        )
    }

    fn part_replay_mismatch(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "REJECT_QATTSVC_PART_REPLAY_MISMATCH",
            message,
        )
    }

    fn commit_incomplete(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "REJECT_QATTSVC_COMMIT_INCOMPLETE",
            message,
        )
    }

    fn commit_mismatch(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "REJECT_QATTSVC_COMMIT_MISMATCH",
            message,
        )
    }

    fn locator_unknown(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "REJECT_QATTSVC_LOCATOR_UNKNOWN",
            message,
        )
    }

    fn fetch_capability(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "REJECT_QATTSVC_FETCH_CAPABILITY",
            message,
        )
    }

    fn range(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "REJECT_QATTSVC_RANGE",
            message,
        )
    }

    fn expired(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, "REJECT_QATTSVC_EXPIRED", message)
    }

    fn policy(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "REJECT_QATTSVC_POLICY",
            message,
        )
    }

    fn quota(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "REJECT_QATTSVC_QUOTA",
            message,
        )
    }

    fn abuse(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "REJECT_QATTSVC_ABUSE",
            message,
        )
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", message)
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response<Body> {
        if self.status.is_server_error() {
            warn!(reason_code = self.reason_code, message = %self.message, "qatt internal error");
        }
        let body = Json(serde_json::json!({
            "reason_code": self.reason_code,
            "message": self.message,
        }));
        (self.status, body).into_response()
    }
}

impl From<io::Error> for ServiceError {
    fn from(value: io::Error) -> Self {
        Self::internal(format!("io failure: {value}"))
    }
}

impl From<serde_json::Error> for ServiceError {
    fn from(value: serde_json::Error) -> Self {
        Self::internal(format!("serialization failure: {value}"))
    }
}

#[derive(Clone)]
struct Storage {
    root: PathBuf,
}

impl Storage {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn ensure_layout(&self) -> io::Result<()> {
        fs::create_dir_all(self.sessions_dir())?;
        fs::create_dir_all(self.objects_dir())?;
        Ok(())
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(session_id)
    }

    fn session_meta_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("session.json")
    }

    fn session_parts_dir(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("parts")
    }

    fn object_dir(&self, locator_ref: &str) -> PathBuf {
        self.objects_dir().join(locator_ref)
    }

    fn object_meta_path(&self, locator_ref: &str) -> PathBuf {
        self.object_dir(locator_ref).join("object.json")
    }

    fn object_bytes_path(&self, locator_ref: &str) -> PathBuf {
        self.object_dir(locator_ref).join("ciphertext.bin")
    }

    fn create_session(&self, session: &SessionMeta) -> io::Result<()> {
        fs::create_dir_all(self.session_parts_dir(&session.session_id))?;
        self.save_session(session)
    }

    fn save_session(&self, session: &SessionMeta) -> io::Result<()> {
        write_json_atomic(&self.session_meta_path(&session.session_id), session)
    }

    fn load_session(&self, session_id: &str) -> io::Result<Option<SessionMeta>> {
        read_json_opt(&self.session_meta_path(session_id))
    }

    fn load_all_sessions(&self) -> io::Result<Vec<SessionMeta>> {
        load_all_json::<SessionMeta>(&self.sessions_dir(), "session.json")
    }

    fn clear_session_parts(&self, session_id: &str) -> io::Result<()> {
        let parts_dir = self.session_parts_dir(session_id);
        if parts_dir.exists() {
            fs::remove_dir_all(parts_dir)?;
        }
        fs::create_dir_all(self.session_parts_dir(session_id))?;
        Ok(())
    }

    fn remove_session(&self, session_id: &str) -> io::Result<()> {
        let dir = self.session_dir(session_id);
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    fn write_part(&self, session_id: &str, part_index: u32, bytes: &[u8]) -> io::Result<()> {
        let path = self
            .session_parts_dir(session_id)
            .join(format!("{part_index}.part"));
        write_bytes_atomic(&path, bytes)
    }

    fn read_part(&self, session_id: &str, part_index: u32) -> io::Result<Option<Vec<u8>>> {
        let path = self
            .session_parts_dir(session_id)
            .join(format!("{part_index}.part"));
        if !path.exists() {
            return Ok(None);
        }
        fs::read(path).map(Some)
    }

    fn read_all_parts(&self, session: &SessionMeta) -> io::Result<Vec<(u32, Vec<u8>)>> {
        let mut parts = Vec::new();
        for index in 0..session.part_count {
            if let Some(bytes) = self.read_part(&session.session_id, index)? {
                parts.push((index, bytes));
            }
        }
        Ok(parts)
    }

    fn create_object(&self, object: &ObjectMeta, ordered_parts: &[Vec<u8>]) -> io::Result<()> {
        let dir = self.object_dir(&object.locator_ref);
        fs::create_dir_all(&dir)?;
        let bytes_path = self.object_bytes_path(&object.locator_ref);
        let mut file = fs::File::create(&bytes_path)?;
        for part in ordered_parts {
            file.write_all(part)?;
        }
        file.flush()?;
        self.save_object(object)
    }

    fn save_object(&self, object: &ObjectMeta) -> io::Result<()> {
        write_json_atomic(&self.object_meta_path(&object.locator_ref), object)
    }

    fn load_object(&self, locator_ref: &str) -> io::Result<Option<ObjectMeta>> {
        read_json_opt(&self.object_meta_path(locator_ref))
    }

    fn load_all_objects(&self) -> io::Result<Vec<ObjectMeta>> {
        load_all_json::<ObjectMeta>(&self.objects_dir(), "object.json")
    }

    fn read_object_bytes(&self, locator_ref: &str) -> io::Result<Vec<u8>> {
        fs::read(self.object_bytes_path(locator_ref))
    }

    fn remove_object_bytes(&self, locator_ref: &str) -> io::Result<()> {
        let bytes_path = self.object_bytes_path(locator_ref);
        if bytes_path.exists() {
            fs::remove_file(bytes_path)?;
        }
        Ok(())
    }

    fn has_active_attachment(&self, attachment_id: &str, now: u64) -> io::Result<bool> {
        for session in self.load_all_sessions()? {
            if session.attachment_id == attachment_id
                && matches!(
                    session.state,
                    SessionState::Created | SessionState::Uploading | SessionState::Committable
                )
                && now <= session.session_expires_at_unix_s
            {
                return Ok(true);
            }
        }
        for object in self.load_all_objects()? {
            if object.attachment_id == attachment_id
                && object.object_state == ObjectState::CommittedObject
                && now <= object.expires_at_unix_s
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

fn write_bytes_atomic(path: &Path, value: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, value)?;
    fs::rename(tmp, path)
}

fn read_json_opt<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    let value =
        serde_json::from_slice(&bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(Some(value))
}

fn load_all_json<T: for<'de> Deserialize<'de>>(dir: &Path, filename: &str) -> io::Result<Vec<T>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join(filename);
        if let Some(value) = read_json_opt(&path)? {
            values.push(value);
        }
    }
    Ok(values)
}

fn reject_noncanonical_query(uri: &Uri) -> Result<(), ServiceError> {
    if uri.query().is_some() {
        return Err(ServiceError::secret_url_placement(
            "canonical URLs must not use query-string carriage",
        ));
    }
    Ok(())
}

fn validate_create_session_request(
    request: &CreateSessionRequest,
    config: &Config,
) -> Result<(), ServiceError> {
    if !is_lower_hex(&request.attachment_id, 64) {
        return Err(ServiceError::session_shape(
            "attachment_id must be 64 lower-case hex chars",
        ));
    }
    if request.ciphertext_len == 0 {
        return Err(ServiceError::session_shape("ciphertext_len must be > 0"));
    }
    if request.ciphertext_len > config.max_ciphertext_bytes {
        return Err(ServiceError::quota(
            "ciphertext_len exceeds configured maximum",
        ));
    }
    if request.part_count == 0 {
        return Err(ServiceError::session_shape("part_count must be > 0"));
    }
    if request.integrity_alg != INTEGRITY_ALG_V1 {
        return Err(ServiceError::session_shape(
            "integrity_alg must equal sha512_merkle_v1",
        ));
    }
    if !is_lower_hex(&request.integrity_root, 128) {
        return Err(ServiceError::session_shape(
            "integrity_root must be 128 lower-case hex chars",
        ));
    }
    let part_size = request.part_size_class.bytes();
    let expected_count = div_ceil(request.ciphertext_len, part_size);
    if request.part_count as u64 != expected_count {
        return Err(ServiceError::session_shape(
            "part_count does not match ciphertext_len / part_size_class",
        ));
    }
    Ok(())
}

fn validate_commit_request(
    request: &CommitRequest,
    session: &SessionMeta,
) -> Result<(), ServiceError> {
    if request.attachment_id != session.attachment_id
        || request.ciphertext_len != session.ciphertext_len
        || request.part_count != session.part_count
        || request.integrity_alg != session.integrity_alg
        || request.integrity_root != session.integrity_root
        || request.retention_class != session.retention_class
    {
        return Err(ServiceError::commit_mismatch(
            "commit body does not match session shape",
        ));
    }
    Ok(())
}

async fn validate_resume_token(
    session: &SessionMeta,
    session_id: &str,
    resume_token: &str,
    state: &AppState,
) -> Result<(), ServiceError> {
    let Some(expected_hash) = session.resume_token_hash.as_deref() else {
        return Err(ServiceError::resume_token(
            "resume token is no longer valid",
        ));
    };
    if hash_secret(resume_token) == expected_hash {
        state
            .inner
            .abuse_tracker
            .lock()
            .await
            .clear_resume_failures(session_id);
        return Ok(());
    }
    let mut abuse = state.inner.abuse_tracker.lock().await;
    if abuse.register_resume_failure(session_id, state.inner.config.invalid_secret_attempt_limit) {
        return Err(ServiceError::abuse("resume-token abuse limit exceeded"));
    }
    Err(ServiceError::resume_token("resume token is invalid"))
}

async fn validate_fetch_capability(
    object: &ObjectMeta,
    locator_ref: &str,
    fetch_capability: &str,
    state: &AppState,
) -> Result<(), ServiceError> {
    let Some(expected_hash) = object.fetch_capability_hash.as_deref() else {
        return Err(ServiceError::fetch_capability(
            "fetch capability is no longer valid",
        ));
    };
    if hash_secret(fetch_capability) == expected_hash {
        state
            .inner
            .abuse_tracker
            .lock()
            .await
            .clear_fetch_failures(locator_ref);
        return Ok(());
    }
    let mut abuse = state.inner.abuse_tracker.lock().await;
    if abuse.register_fetch_failure(locator_ref, state.inner.config.invalid_secret_attempt_limit) {
        return Err(ServiceError::abuse("fetch-capability abuse limit exceeded"));
    }
    Err(ServiceError::fetch_capability(
        "fetch capability is invalid",
    ))
}

fn extract_required_secret(headers: &HeaderMap, name: &str) -> Result<String, ServiceError> {
    let Some(value) = headers.get(name) else {
        return Err(match name {
            RESUME_TOKEN_HEADER => ServiceError::resume_token("missing resume token header"),
            FETCH_CAPABILITY_HEADER => {
                ServiceError::fetch_capability("missing fetch capability header")
            }
            _ => ServiceError::internal("missing header"),
        });
    };
    let value = value
        .to_str()
        .map_err(|_| ServiceError::secret_url_placement("secret header must be valid ASCII"))?;
    if !is_base64url_token(value, 32, 255) {
        return Err(match name {
            RESUME_TOKEN_HEADER => ServiceError::resume_token("resume token is malformed"),
            FETCH_CAPABILITY_HEADER => {
                ServiceError::fetch_capability("fetch capability is malformed")
            }
            _ => ServiceError::internal("malformed secret header"),
        });
    }
    Ok(value.to_owned())
}

fn validate_non_secret_ref(value: &str) -> Result<(), String> {
    if is_base64url_token(value, 1, 128) {
        Ok(())
    } else {
        Err("path reference must be a non-secret base64url token".to_owned())
    }
}

fn expected_part_length(session: &SessionMeta, part_index: u32) -> u64 {
    let part_size = session.part_size_class.bytes();
    if part_index + 1 < session.part_count {
        part_size
    } else {
        session.ciphertext_len - (part_size * u64::from(session.part_count - 1))
    }
}

fn missing_part_ranges(session: &SessionMeta) -> Vec<MissingRange> {
    let mut missing = Vec::new();
    let mut current_start: Option<u32> = None;
    for index in 0..session.part_count {
        let present = session.stored_parts.contains_key(&index);
        match (present, current_start) {
            (false, None) => current_start = Some(index),
            (true, Some(start)) => {
                missing.push(MissingRange {
                    start,
                    end: index - 1,
                });
                current_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = current_start {
        missing.push(MissingRange {
            start,
            end: session.part_count - 1,
        });
    }
    missing
}

#[derive(Debug, Clone, Copy)]
struct ByteRange {
    start: u64,
    end: u64,
}

fn parse_single_range(header: &str, total_len: u64) -> Result<ByteRange, ServiceError> {
    if !header.starts_with("bytes=") {
        return Err(ServiceError::range("range header must start with bytes="));
    }
    let raw = &header[6..];
    if raw.contains(',') {
        return Err(ServiceError::range("multiple ranges are not supported"));
    }
    let (start_raw, end_raw) = raw
        .split_once('-')
        .ok_or_else(|| ServiceError::range("range header must be bytes=start-end"))?;
    if start_raw.is_empty() || end_raw.is_empty() {
        return Err(ServiceError::range("range header must be bytes=start-end"));
    }
    let start: u64 = start_raw
        .parse()
        .map_err(|_| ServiceError::range("range start is invalid"))?;
    let end: u64 = end_raw
        .parse()
        .map_err(|_| ServiceError::range("range end is invalid"))?;
    if start > end || end >= total_len {
        return Err(ServiceError::range(
            "range must fit within committed ciphertext length",
        ));
    }
    Ok(ByteRange { start, end })
}

pub fn sha512_merkle_root(parts: &[Vec<u8>]) -> String {
    let mut level: Vec<[u8; 64]> = parts
        .iter()
        .enumerate()
        .map(|(index, bytes)| leaf_hash(index as u32, bytes))
        .collect();
    if level.is_empty() {
        return hex::encode([0u8; 64]);
    }
    while level.len() > 1 {
        if level.len() % 2 == 1 {
            let last = *level.last().expect("non-empty");
            level.push(last);
        }
        let mut next = Vec::with_capacity(level.len() / 2);
        for pair in level.chunks(2) {
            let mut hasher = Sha512::new();
            hasher.update([0x01]);
            hasher.update(pair[0]);
            hasher.update(pair[1]);
            let digest = hasher.finalize();
            let mut array = [0u8; 64];
            array.copy_from_slice(&digest);
            next.push(array);
        }
        level = next;
    }
    hex::encode(level[0])
}

fn leaf_hash(index: u32, bytes: &[u8]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update([0x00]);
    hasher.update(index.to_be_bytes());
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 64];
    out.copy_from_slice(&digest);
    out
}

fn hash_secret(secret: &str) -> String {
    hex::encode(Sha512::digest(secret.as_bytes()))
}

fn audit_handle(namespace: &str, raw: &str) -> String {
    let digest = Sha512::digest(format!("qatt.audit.v1|{namespace}|{raw}").as_bytes());
    hex::encode(digest)[..12].to_string()
}

fn audit_handle_opt(namespace: &str, raw: Option<&str>) -> Option<String> {
    raw.map(|value| audit_handle(namespace, value))
}

fn random_token(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn is_base64url_token(value: &str, min_len: usize, max_len: usize) -> bool {
    let len = value.len();
    len >= min_len
        && len <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn div_ceil(lhs: u64, rhs: u64) -> u64 {
    lhs.div_ceil(rhs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merkle_root_is_stable() {
        let parts = vec![b"abc".to_vec(), b"def".to_vec()];
        let root = sha512_merkle_root(&parts);
        assert_eq!(root.len(), 128);
    }
}
