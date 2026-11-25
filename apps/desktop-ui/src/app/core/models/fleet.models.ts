export type TransportType = 'local' | 'lan' | 'relay';
export type NodeStatus = 'online' | 'degraded' | 'offline';
export type AgentType = 'claude' | 'codex' | 'opencode' | 'pi' | 'grok' | 'antigravity';
export type DifficultyTier = 'L1' | 'L2' | 'L3' | 'L4';
export type RoutingMode = 'cost' | 'balanced' | 'quality';
export type WorkflowTemplate = 'direct' | 'implement_verify_review' | 'planner_dag';
export type TaskRole = 'planner' | 'worker' | 'reviewer' | 'shell';
export type ExecutionMode = 'task' | 'terminal';
export type SlotHealth = 'healthy' | 'rate_limited' | 'auth_expired' | 'disabled';
export type MissionStatus = 'draft' | 'running' | 'paused' | 'completed' | 'failed' | 'cancelled';
export type TaskStatus =
  | 'pending'
  | 'queued'
  | 'running'
  | 'waiting_input'
  | 'waiting_approval'
  | 'completed'
  | 'failed'
  | 'cancelled';
export type AttentionKind =
  | 'waiting_input'
  | 'waiting_approval'
  | 'rate_limited'
  | 'auth_expired'
  | 'task_failed';

export interface FleetNode {
  id: string;
  name: string;
  transportType: TransportType;
  endpoint: string;
  allowInsecureTls?: boolean;
  status: NodeStatus;
  tags: string[];
  maxConcurrency: number;
  lastHeartbeatMs: number | null;
  lastError: string | null;
}

export interface NodeProbeResult {
  reachable: boolean;
  latencyMs: number;
  workspaceCount: number;
  terminalCount: number;
  supportsAccountRouting: boolean;
  supportsMcpProfiles: boolean;
  error: string | null;
}

export interface ExecutionRecord {
  id: string;
  missionId: string;
  taskId: string;
  taskTitle: string;
  nodeId: string;
  slotId: string | null;
  agentType: AgentType | null;
  modelName: string;
  difficulty: DifficultyTier;
  status: TaskStatus;
  startedAtMs: number;
  finishedAtMs: number;
  durationMs: number;
  retryCount: number;
  escalated: boolean;
  estimatedCostUnits: number;
}

export interface AnalyticsBucket {
  key: string;
  total: number;
  completed: number;
  failed: number;
  avgDurationMs: number;
  costUnits: number;
}

export interface FleetAnalytics {
  totalRuns: number;
  completedRuns: number;
  failedRuns: number;
  successRatePercent: number;
  avgDurationMs: number;
  escalatedRuns: number;
  totalCostUnits: number;
  baselineL3CostUnits: number;
  estimatedSavingsPercent: number;
  byTier: AnalyticsBucket[];
  byModel: AnalyticsBucket[];
  byNode: AnalyticsBucket[];
}

export interface NotificationSettings {
  webhookEnabled: boolean;
  webhookUrl: string;
  wechatViaTermexo: boolean;
  notifyOnCompleted: boolean;
  notifyOnFailed: boolean;
  notifyOnApproval: boolean;
  notifyOnSlotUnhealthy: boolean;
}

export interface TermexoWorkspaceSummary {
  id: string;
  name: string;
  projectPath: string;
  terminalIds: string[];
}

export interface TermexoTerminalSummary {
  id: string;
  workspaceId: string;
  name: string;
  agentType: string;
  model: string;
  status: string;
  live: boolean;
  accountProfileId?: string | null;
}

export interface TermexoAccountProfileSummary {
  id: string;
  name: string;
  agentType: string;
  authenticated: boolean;
  isDefault: boolean;
  isSystem: boolean;
}

export interface TermexoModelProfileSummary {
  id: string;
  name: string;
  provider: string;
  isDefault: boolean;
  hasCredential: boolean;
  claudeEnabled: boolean;
  claudeModel: string;
  codexEnabled: boolean;
  codexModel: string;
}

export interface TermexoMcpProfileSummary {
  id: string;
  name: string;
}

export interface TermexoSettingsSnapshot {
  modelProfiles: TermexoModelProfileSummary[];
  accountProfiles: TermexoAccountProfileSummary[];
  mcpProfiles: TermexoMcpProfileSummary[];
}

export interface NodeLiveSnapshot {
  nodeId: string;
  workspaces: TermexoWorkspaceSummary[];
  terminals: TermexoTerminalSummary[];
  settings: TermexoSettingsSnapshot;
  syncedAtMs: number;
}

export interface AccountModelSlot {
  id: string;
  nodeId: string;
  label: string;
  agentType: AgentType;
  accountProfileId: string | null;
  modelProfileId: string | null;
  modelName: string;
  modelFamily: string;
  supportedTiers: DifficultyTier[];
  costTier: number;
  qualityTier: number;
  maxConcurrency: number;
  activeCount: number;
  priority: number;
  health: SlotHealth;
  cooldownUntilMs: number | null;
}

export interface GuardrailPolicy {
  maxDepth: number;
  maxConcurrency: number;
  maxSubagents: number;
  isolateWorktreeByDefault: boolean;
}

export interface Mission {
  id: string;
  title: string;
  goalPrompt: string;
  nodeId: string;
  workspaceId: string;
  projectId: string;
  routingMode: RoutingMode;
  workflowTemplate: WorkflowTemplate;
  requiredNodeTags: string[];
  guardrails: GuardrailPolicy;
  fleetMcpProfileId: string | null;
  status: MissionStatus;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface MissionTask {
  id: string;
  missionId: string;
  parentTaskId: string | null;
  depth: number;
  dependsOn: string[];
  requiredNodeTags: string[];
  role: TaskRole;
  executionMode: ExecutionMode;
  preferredTerminalId: string | null;
  title: string;
  description: string;
  acceptanceCriteria: string;
  difficulty: DifficultyTier;
  autoDifficulty: boolean;
  preferredAgent: AgentType | null;
  excludeModelFamily: string | null;
  assignedNodeId: string | null;
  assignedSlotId: string | null;
  assignedAgentType: AgentType | null;
  assignedModelName: string | null;
  assignedAccountProfileId: string | null;
  routingReason: string | null;
  termexoTaskId: string | null;
  termexoTerminalId: string | null;
  terminalStopRequested?: boolean;
  worktreePath: string | null;
  worktreeBranch: string | null;
  status: TaskStatus;
  retryCount: number;
  maxRetries: number;
  resultSummary: string | null;
  diffStat: string | null;
  lastError: string | null;
  parentToken: string;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface AttentionItem {
  id: string;
  nodeId: string;
  nodeName: string;
  workspaceId: string | null;
  terminalId: string | null;
  missionTaskId: string | null;
  title: string;
  kind: AttentionKind;
  detail: string;
  detectedAtMs: number;
}

export interface FleetStateOverview {
  nodes: FleetNode[];
  snapshots: Record<string, NodeLiveSnapshot>;
  slots: AccountModelSlot[];
  missions: Mission[];
  tasks: MissionTask[];
  attention: AttentionItem[];
  executionRecords: ExecutionRecord[];
  analytics: FleetAnalytics;
  notificationSettings: NotificationSettings;
  mcpServerUrl: string;
  mcpRemoteUrl: string | null;
  mcpServerToken: string;
}

export interface UpsertNodeInput {
  id?: string;
  name: string;
  transportType: TransportType;
  endpoint: string;
  token?: string;
  tags: string[];
  maxConcurrency: number;
  allowInsecureTls?: boolean;
}

export interface CreateMissionInput {
  title: string;
  goalPrompt: string;
  nodeId: string;
  workspaceId: string;
  projectId: string;
  routingMode: RoutingMode;
  workflowTemplate: WorkflowTemplate;
  requiredNodeTags: string[];
  executionMode: ExecutionMode;
  preferredTerminalId: string | null;
  difficulty: DifficultyTier | null;
  preferredAgent: AgentType | null;
  guardrails: GuardrailPolicy;
  fleetMcpProfileId: string | null;
  autoDispatch: boolean;
}
