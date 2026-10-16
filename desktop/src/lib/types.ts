export type Mode = 'send' | 'receive' | 'both';
export interface Profile {
  name: string;
  mode: Mode;
  chunk_size: number;
  frame_ms: number;
  correction: string;
  max_version: number;
  qr_pixels: number;
  manifest_interval: number;
  policy: 'once' | 'loop' | 'until_acknowledged' | 'directory_cycle';
  feedback: boolean;
  ack_frames: boolean;
  ack_files: boolean;
  stop_on_ack: boolean;
  allow_unpaired_ack: boolean;
  trusted_peers: string[];
  destination: string;
  conflict: 'rename' | 'skip_identical' | 'overwrite';
  preserve_paths: boolean;
  control_interval: number;
  module_pixels: number;
  display_name: string | null;
  quiet_zone: number;
  foreground: string;
  background: string;
  inverted: boolean;
  camera_mirror: boolean;
  camera_rotation: number;
  camera_crop: [number, number, number, number] | null;
  camera_zoom: number;
  camera_index: number;
  camera_width: number;
  camera_height: number;
  camera_fps: number;
  max_receive_bytes: number;
}
export interface FileEntry {
  id: number;
  path: string;
  size: number;
  sha256: string;
  chunks: number;
  directory: boolean;
}
export interface Frame {
  session: string;
  transfer: string;
  kind: 'Manifest' | 'Data' | 'Control' | 'Bundle';
  file: number;
  index: number;
  total: number;
  offset: number;
  payload: number[];
}
export interface Job {
  id: string;
  session: string;
  direction: string;
  status: string;
  priority: number;
  created: number;
  manifest: { version: number; transfer: string; chunk_size: number; files: FileEntry[] };
  manifest_hash: string;
  profile: Profile;
  cycle: number;
  frames: number;
  bytes: number;
  duplicates: number;
  rejected: number;
  delivered_files: number[];
  confirmed_files: number[];
  last_frame: Frame | null;
  last_activity: number;
  error: string | null;
  password_bundle: boolean;
  password_needed: boolean;
  bundle_files: FileEntry[];
}
export interface DisplayFrame {
  duration_ms: number;
  text: string;
  svg: string;
  frame: Frame;
  cycle: number;
  checksum: string;
}
export interface FileProgress {
  file: number;
  received_chunks: number;
  total_chunks: number;
  received_bytes: number;
  verified: boolean;
  path: string | null;
}
export interface TransferView {
  job: Job;
  files: FileProgress[];
  estimated_loop_seconds: number;
  useful_bytes_per_second: number;
  eta_seconds: number | null;
}
export interface PayloadPage {
  offset: number;
  size: number;
  bytes: (number | null)[];
  verified: boolean;
  ranges: [number, number][];
}
export type DisplaySettings = Partial<Pick<Profile, 'frame_ms' | 'qr_pixels' | 'quiet_zone' | 'inverted'>>;
export interface CameraStatus {
  capture_fps?: number;
  scan_fps?: number;
  scan_ms?: number;
  device_fps?: number;
  running?: boolean;
  image?: string;
  captured?: number;
  decoded?: number;
  error?: string;
  reconnecting?: boolean;
  controls?: { name: string; label: string; value: Record<string, unknown>; description: unknown; flags: string[] }[];
  controlError?: string;
}
export interface Status {
  version: string;
  buildHash: string;
  mode: Mode;
  fingerprint: string;
  publicKey: string;
  profile: Profile;
  camera: CameraStatus | null;
}
export interface TransferEvent {
  id: string;
  event: string;
  transfer: string;
  file: number | null;
  timestamp: number;
  data: Record<string, unknown>;
}
