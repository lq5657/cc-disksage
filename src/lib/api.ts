import { Channel, invoke } from "@tauri-apps/api/core";

export interface VolumeInfo {
  letter: string;
  label: string;
  filesystem: string;
  total_bytes: number;
  free_bytes: number;
}

export interface DirNode {
  name: string;
  path: string;
  allocated_bytes: number;
  logical_bytes: number;
  file_count: number;
  children: DirNode[];
}

export interface LargeFile {
  path: string;
  allocated_bytes: number;
  logical_bytes: number;
  modified_at: number | null;
  extension: string;
}

export type RiskLevel = "safe" | "caution" | "danger";
export type Recoverability =
  | "regenerable"
  | "recycle_bin"
  | "quarantine"
  | "irreversible"
  | "user_data";

export interface Evidence {
  id: string;
  source: "rule" | "scanner" | "app_detection" | "llm";
  kind:
    | "rule_provenance"
    | "path"
    | "size"
    | "age"
    | "file_type"
    | "safety_check"
    | "app_detection"
    | "llm_inference";
  claim: string;
  path: string | null;
  bytes: number | null;
  timestamp: number | null;
  rule_id: string | null;
  confidence: number;
}

export interface SampleFile {
  name: string;
  bytes: number;
  modified_at: number | null;
}

export interface FindingAction {
  type: string;
  [key: string]: unknown;
}

export interface Finding {
  id: string;
  rule_id: string;
  rule_version: number;
  category: string;
  title_zh: string;
  title_en: string;
  risk: RiskLevel;
  recoverability: Recoverability;
  requires_admin: boolean;
  default_selected: boolean;
  allocated_bytes: number;
  logical_bytes: number;
  file_count: number;
  size_provenance: "scanner" | "helper" | "estimated";
  oldest_mtime: number | null;
  newest_mtime: number | null;
  ext_histogram: Array<[string, number]>;
  sample_files: SampleFile[];
  evidence: Evidence[];
  consequence: string;
  action: FindingAction;
  llm: unknown | null;
}

export interface ScanSnapshot {
  id: string;
  volume: string;
  engine: string;
  started_at: number;
  duration_ms: number;
  total_bytes: number;
  allocated_bytes: number;
  free_bytes: number;
  file_count: number;
  dir_count: number;
  error_count: number;
  node_limit_hit: boolean;
  top_dirs: DirNode[];
  large_files: LargeFile[];
}

export async function getVolumes(): Promise<VolumeInfo[]> {
  return invoke<VolumeInfo[]>("get_volumes");
}

export interface ScanProgress {
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  elapsed_ms: number;
  current: string;
}

export async function scanVolume(
  volume: string,
  onProgress?: (progress: ScanProgress) => void,
): Promise<ScanSnapshot> {
  const channel = new Channel<ScanProgress>();
  if (onProgress) channel.onmessage = onProgress;
  return invoke<ScanSnapshot>("scan_volume", { volume, onProgress: channel });
}

export async function cancelScan(): Promise<void> {
  return invoke("cancel_scan");
}

export async function scanVolumeMft(volume: string): Promise<ScanSnapshot> {
  return invoke<ScanSnapshot>("scan_volume_mft", { volume });
}

export interface DiscoveryProgress {
  files: number;
  dirs: number;
  elapsed_ms: number;
  current: string;
}

export interface DiscoveryItem {
  index: number;
  path: string;
  name: string;
  bytes: number;
  modified_at: number | null;
  extension: string;
  category:
    | "large_file"
    | "large_log"
    | "dump"
    | "old_installer"
    | "old_archive";
  reason_zh: string;
  reason_en: string;
}

export interface DiscoveryResult {
  volume: string;
  scanned_files: number;
  scanned_dirs: number;
  elapsed_ms: number;
  items: DiscoveryItem[];
  truncated: boolean;
}

export async function deepInspect(
  volume: string,
  onProgress?: (progress: DiscoveryProgress) => void,
): Promise<DiscoveryResult> {
  const channel = new Channel<DiscoveryProgress>();
  if (onProgress) channel.onmessage = onProgress;
  return invoke<DiscoveryResult>("deep_inspect", { volume, onProgress: channel });
}

export async function cancelDeepInspect(): Promise<void> {
  return invoke("cancel_deep_inspect");
}

export async function openDiscoveryItem(index: number): Promise<void> {
  return invoke("open_discovery_item", { index });
}

export interface ScanHistoryEntry {
  id: string;
  volume: string;
  engine: string;
  started_at: number;
  duration_ms: number;
  file_count: number;
  dir_count: number;
  allocated_bytes: number;
  free_bytes: number;
  error_count: number;
  node_limit_hit: boolean;
}

export async function getLatestScan(): Promise<ScanSnapshot | null> {
  return invoke<ScanSnapshot | null>("get_latest_scan");
}

export async function getScanHistory(limit = 20): Promise<ScanHistoryEntry[]> {
  return invoke<ScanHistoryEntry[]>("get_scan_history", { limit });
}

export async function getLatestDuplicateScan(): Promise<DuplicateScanResult | null> {
  return invoke<DuplicateScanResult | null>("get_latest_duplicate_scan");
}

export interface DuplicateScanProgress {
  phase: string;
  files_seen: number;
  candidate_files: number;
  hashed_files: number;
  groups_found: number;
  bytes_hashed: number;
  elapsed_ms: number;
  current: string;
}

export interface DuplicateFile {
  id: string;
  path: string;
  modified_at: number | null;
  allocated_bytes: number;
}

export interface DuplicateGroup {
  id: string;
  size: number;
  hash: string;
  files: DuplicateFile[];
  wasted_bytes: number;
}

export interface DuplicateScanResult {
  scan_id: string;
  roots: string[];
  groups: DuplicateGroup[];
  total_wasted_bytes: number;
  files_scanned: number;
  candidate_files: number;
  hashed_files: number;
  error_count: number;
  duration_ms: number;
  truncated: boolean;
  min_size: number;
}

export async function findDuplicateFiles(
  scope: string,
  minSize: number,
  onProgress?: (progress: DuplicateScanProgress) => void,
): Promise<DuplicateScanResult> {
  const channel = new Channel<DuplicateScanProgress>();
  if (onProgress) channel.onmessage = onProgress;
  return invoke<DuplicateScanResult>("find_duplicate_files", {
    scope,
    minSize,
    onProgress: channel,
  });
}

export async function cancelDuplicateScan(): Promise<void> {
  return invoke("cancel_duplicate_scan");
}

export async function deleteDuplicateFiles(
  scanId: string,
  groupId: string,
  keepFileId: string,
  deleteFileIds: string[],
): Promise<CleanupResult> {
  return invoke<CleanupResult>("delete_duplicate_files", {
    scanId,
    groupId,
    keepFileId,
    deleteFileIds,
  });
}

export async function openDuplicateFile(
  scanId: string,
  fileId: string,
): Promise<void> {
  return invoke("open_duplicate_file", { scanId, fileId });
}

export async function quickScan(): Promise<Finding[]> {
  return invoke<Finding[]>("quick_scan");
}

export interface AdminScanResponse {
  findings: Finding[];
  admin_error: string | null;
  admin_count: number;
}

export async function quickScanAdmin(): Promise<AdminScanResponse> {
  return invoke<AdminScanResponse>("quick_scan_admin");
}

export async function getFinding(findingId: string): Promise<Finding> {
  return invoke<Finding>("get_finding", { findingId });
}

export interface TreemapNode {
  name: string;
  path: string;
  value: number;
  file_count: number;
  has_children: boolean;
  children: TreemapNode[];
}

export interface TreemapResponse {
  root: TreemapNode;
  total_bytes: number;
  file_count: number;
  truncated: boolean;
  full: boolean;
}

export interface TreemapChildrenResponse {
  path: string;
  name: string;
  value: number;
  file_count: number;
  children: TreemapNode[];
  truncated: boolean;
  full: boolean;
}

export async function getTreemap(snapshotId: string): Promise<TreemapResponse> {
  return invoke<TreemapResponse>("get_treemap", { snapshotId });
}

export async function getTreemapChildren(
  snapshotId: string,
  path: string,
): Promise<TreemapChildrenResponse> {
  return invoke<TreemapChildrenResponse>("get_treemap_children", {
    snapshotId,
    path,
  });
}

export interface AuditEntry {
  id: string;
  plan_id: string;
  finding_id: string;
  rule_id: string;
  action: string;
  paths: string[];
  bytes: number;
  result: string;
  timestamp: number;
  elevation: boolean;
}

export async function getAuditHistory(limit = 100): Promise<AuditEntry[]> {
  return invoke<AuditEntry[]>("get_audit_history", { limit });
}

export interface PlanWarning {
  code: string;
  message_zh: string;
  message_en: string;
}

export interface PlanPreviewItem {
  finding_id: string;
  rule_id: string;
  title_zh: string;
  risk: RiskLevel;
  bytes: number;
  file_count: number;
  recoverability: Recoverability;
  requires_admin: boolean;
  size_provenance: "scanner" | "helper" | "estimated";
  action: string;
}

export interface PlanPreview {
  plan_id: string;
  total_bytes: number;
  file_count: number;
  items: PlanPreviewItem[];
  warnings: PlanWarning[];
}

export interface CleanupResult {
  job_id: string;
  freed_bytes: number;
  deleted: number;
  skipped_locked: string[];
  failed: Array<[string, string]>;
  cancelled: boolean;
  notes: string[];
}

export interface CleanupProgress {
  phase: string;
  processed: number;
  total: number;
  deleted: number;
  failed: number;
  bytes: number;
  current: string;
  elapsed_ms: number;
}

export async function createPlan(findingIds: string[]): Promise<PlanPreview> {
  return invoke<PlanPreview>("create_plan", { findingIds });
}

export async function executePlan(
  planId: string,
  permanentDelete: boolean,
  onProgress?: (progress: CleanupProgress) => void,
): Promise<CleanupResult> {
  const channel = new Channel<CleanupProgress>();
  if (onProgress) channel.onmessage = onProgress;
  return invoke<CleanupResult>("execute_plan", {
    planId,
    permanentDelete,
    onProgress: channel,
  });
}

export interface LargeFileDeleteResponse {
  snapshot: ScanSnapshot;
  result: CleanupResult;
}

export async function openLargeFile(
  snapshotId: string,
  fileIndex: number,
): Promise<void> {
  return invoke("open_large_file", { snapshotId, fileIndex });
}

export async function deleteLargeFile(
  snapshotId: string,
  fileIndex: number,
  confirmed: boolean,
): Promise<LargeFileDeleteResponse> {
  return invoke<LargeFileDeleteResponse>("delete_large_file", {
    snapshotId,
    fileIndex,
    confirmed,
  });
}

export async function llmPreviewLargeFile(
  snapshotId: string,
  fileIndex: number,
): Promise<unknown> {
  return invoke<unknown>("llm_preview_large_file", { snapshotId, fileIndex });
}

export async function llmAssessLargeFile(
  snapshotId: string,
  fileIndex: number,
): Promise<LlmAssessment> {
  return invoke<LlmAssessment>("llm_assess_large_file", { snapshotId, fileIndex });
}

export interface LlmPreset {
  id: string;
  name_zh: string;
  base_url: string;
  model: string;
}

export interface LlmConfigPublic {
  provider_id: string;
  base_url: string;
  model: string;
  temperature: number;
  max_tokens: number;
  timeout_secs: number;
  redact_paths: boolean;
  send_sample_names: boolean;
  sample_name_limit: number;
  has_key: boolean;
  presets: LlmPreset[];
}

export interface LlmConfigInput {
  provider_id: string;
  base_url: string;
  model: string;
  temperature: number;
  max_tokens: number;
  timeout_secs: number;
  redact_paths: boolean;
  send_sample_names: boolean;
  sample_name_limit: number;
}

export interface LlmAssessment {
  schema_version: number;
  finding_id: string;
  verdict: "cleanable" | "review" | "do_not_clean";
  confidence: "low" | "medium" | "high";
  summary: string;
  reasoning: Array<{ text: string; evidence_ids: string[] }>;
  cited_evidence_ids: string[];
  consequence: string;
  caveats: string[];
  model: string;
  generated_at: number;
  payload_hash: string;
}

export async function llmGetConfig(): Promise<LlmConfigPublic> {
  return invoke<LlmConfigPublic>("llm_get_config");
}

export async function llmSetConfig(config: LlmConfigInput): Promise<LlmConfigPublic> {
  return invoke<LlmConfigPublic>("llm_set_config", { config });
}

export async function llmSetKey(
  providerId: string,
  apiKey: string,
): Promise<void> {
  return invoke("llm_set_key", { providerId, apiKey });
}

export async function llmClearKey(providerId: string): Promise<void> {
  return invoke("llm_clear_key", { providerId });
}

export async function llmTestConnection(): Promise<void> {
  return invoke("llm_test_connection");
}

export async function llmPreviewPayload(findingId: string): Promise<unknown> {
  return invoke("llm_preview_payload", { findingId });
}

export async function llmAssessFinding(findingId: string): Promise<LlmAssessment> {
  return invoke<LlmAssessment>("llm_assess_finding", { findingId });
}

export async function llmAskFollowup(
  findingId: string,
  question: string,
): Promise<string> {
  return invoke<string>("llm_ask_followup", { findingId, question });
}

export interface AutoCleanConfig {
  enabled: boolean;
  frequency: string;
  min_free_percent: number;
  permanent_delete: boolean;
  last_run: number | null;
  last_freed_bytes: number;
  last_deleted: number;
  last_status: string;
}

export interface AutoCleanStatus {
  config: AutoCleanConfig;
  task_registered: boolean;
  task_name: string;
  executable: string | null;
}

export async function getAutoCleanStatus(): Promise<AutoCleanStatus> {
  return invoke<AutoCleanStatus>("get_auto_clean_status");
}

export async function setAutoCleanConfig(
  config: AutoCleanConfig,
): Promise<AutoCleanStatus> {
  return invoke<AutoCleanStatus>("set_auto_clean_config", { config });
}

export async function runAutoCleanNow(): Promise<CleanupResult> {
  return invoke<CleanupResult>("run_auto_clean_now");
}
