use qrtx_protocol::{Frame, Manifest};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Send,
    Receive,
    Both,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    Once,
    Loop,
    UntilAcknowledged,
    DirectoryCycle,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    Rename,
    SkipIdentical,
    Overwrite,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub mode: Mode,
    pub chunk_size: u32,
    pub frame_ms: u64,
    pub correction: String,
    pub max_version: i16,
    pub qr_pixels: u32,
    pub manifest_interval: u32,
    pub policy: Policy,
    pub feedback: bool,
    pub ack_frames: bool,
    pub ack_files: bool,
    pub stop_on_ack: bool,
    pub allow_unpaired_ack: bool,
    pub trusted_peers: Vec<String>,
    pub destination: PathBuf,
    pub conflict: Conflict,
    pub preserve_paths: bool,
    pub camera_index: u32,
    pub camera_width: u32,
    pub camera_height: u32,
    pub camera_fps: u32,
    pub max_receive_bytes: u64,
    pub control_interval: u32,
    pub module_pixels: u32,
    pub display_name: Option<String>,
    pub quiet_zone: u32,
    pub foreground: String,
    pub background: String,
    pub inverted: bool,
    pub camera_mirror: bool,
    pub camera_rotation: u16,
    pub camera_crop: Option<[u32; 4]>,
    pub camera_zoom: f32,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            control_interval: 4,
            module_pixels: 0,
            display_name: None,
            quiet_zone: 4,
            foreground: "#000000".into(),
            background: "#ffffff".into(),
            inverted: false,
            camera_mirror: false,
            camera_rotation: 0,
            camera_crop: None,
            camera_zoom: 1.0,
            name: "Reliable desktop".into(),
            mode: Mode::Both,
            chunk_size: 600,
            frame_ms: 250,
            correction: "M".into(),
            max_version: 40,
            qr_pixels: 640,
            manifest_interval: 32,
            policy: Policy::Loop,
            feedback: true,
            ack_frames: true,
            ack_files: true,
            stop_on_ack: true,
            allow_unpaired_ack: true,
            trusted_peers: vec![],
            destination: PathBuf::from("received"),
            conflict: Conflict::Rename,
            preserve_paths: true,
            camera_index: 0,
            camera_width: 1280,
            camera_height: 720,
            camera_fps: 30,
            max_receive_bytes: 16 * 1024 * 1024 * 1024,
        }
    }
}
impl Profile {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (64..=1168).contains(&self.chunk_size) && (50..=60_000).contains(&self.frame_ms),
            "invalid chunk size or frame duration"
        );
        anyhow::ensure!(
            (1..=1024).contains(&self.manifest_interval) && (128..=4096).contains(&self.qr_pixels),
            "invalid display settings"
        );
        anyhow::ensure!(
            (1..=4096).contains(&self.camera_width)
                && (1..=2160).contains(&self.camera_height)
                && (1..=120).contains(&self.camera_fps),
            "camera limits exceeded"
        );
        anyhow::ensure!(
            self.max_receive_bytes > 0 && self.max_receive_bytes <= qrtx_protocol::MAX_FILE_SIZE,
            "invalid receive quota"
        );
        anyhow::ensure!(
            (4..=16).contains(&self.quiet_zone)
                && [0, 90, 180, 270].contains(&self.camera_rotation)
                && (1.0..=4.0).contains(&self.camera_zoom),
            "invalid quiet zone or camera transform"
        );
        anyhow::ensure!(
            (2..=64).contains(&self.control_interval) && self.module_pixels <= 32,
            "invalid control allocation or module size"
        );
        for color in [&self.foreground, &self.background] {
            anyhow::ensure!(
                color.len() == 7 && color.starts_with('#') && hex::decode(&color[1..]).is_ok(),
                "colors require #RRGGBB"
            );
        }
        for p in &self.trusted_peers {
            anyhow::ensure!(
                p.len() == 64 && hex::decode(p).is_ok(),
                "invalid peer fingerprint"
            );
        }
        let frame = Frame {
            session: Uuid::nil(),
            transfer: Uuid::nil(),
            kind: qrtx_protocol::Kind::Manifest,
            file: 0,
            index: 0,
            total: 1,
            offset: 0,
            payload: vec![255; self.chunk_size as usize + 32],
        };
        if qrtx_protocol::qr(&frame.encode()?, &self.correction, self.max_version).is_err() {
            anyhow::bail!(
                "QR density exceeded: maximum chunk size for this configuration is {} bytes",
                qrtx_protocol::maximum_chunk(&self.correction, self.max_version)
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub session: Uuid,
    pub direction: String,
    pub status: String,
    pub priority: i32,
    pub created: u64,
    pub manifest: Manifest,
    pub manifest_hash: String,
    pub profile: Profile,
    pub source_paths: Vec<PathBuf>,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub password_bundle: bool,
    #[serde(default)]
    pub password_needed: bool,
    /// Authenticated inner entries. Local IDs never collide with an outer file ID.
    #[serde(default)]
    pub bundle_files: Vec<qrtx_protocol::FileEntry>,
    pub cycle: u64,
    pub frames: u64,
    pub bytes: u64,
    pub cursor_file: usize,
    pub cursor_chunk: u32,
    pub manifest_cursor: u32,
    pub since_manifest: u32,
    pub delivered_files: Vec<u32>,
    pub confirmed_files: Vec<u32>,
    pub duplicates: u64,
    pub rejected: u64,
    pub last_frame: Option<Frame>,
    pub last_activity: u64,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub event: String,
    pub transfer: Uuid,
    pub file: Option<u32>,
    pub timestamp: u64,
    pub data: serde_json::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileProgress {
    pub file: u32,
    pub received_chunks: u64,
    pub total_chunks: u32,
    pub received_bytes: u64,
    pub verified: bool,
    pub path: Option<String>,
    pub received_set_digest: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferView {
    pub job: Job,
    pub files: Vec<FileProgress>,
    pub estimated_loop_seconds: f64,
    pub useful_bytes_per_second: f64,
    pub eta_seconds: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayFrame {
    pub duration_ms: u64,
    pub text: String,
    pub svg: String,
    pub frame: Frame,
    pub cycle: u64,
    pub checksum: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayloadPage {
    pub offset: u64,
    pub size: u64,
    pub bytes: Vec<Option<u8>>,
    pub verified: bool,
    pub ranges: Vec<(u64, u64)>,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySettings {
    pub frame_ms: Option<u64>,
    pub qr_pixels: Option<u32>,
    pub quiet_zone: Option<u32>,
    pub inverted: Option<bool>,
}
impl DisplaySettings {
    pub fn apply(&self, profile: &mut Profile) -> anyhow::Result<()> {
        if let Some(value) = self.frame_ms {
            profile.frame_ms = value;
        }
        if let Some(value) = self.qr_pixels {
            profile.qr_pixels = value;
            profile.module_pixels = 0;
        }
        if let Some(value) = self.quiet_zone {
            profile.quiet_zone = value;
        }
        if let Some(value) = self.inverted {
            profile.inverted = value;
        }
        profile.validate()
    }
}
