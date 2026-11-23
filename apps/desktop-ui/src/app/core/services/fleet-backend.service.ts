import { Injectable, computed, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AccountModelSlot,
  CreateMissionInput,
  DifficultyTier,
  FleetNode,
  FleetStateOverview,
  Mission,
  MissionTask,
  NodeProbeResult,
  NotificationSettings,
  UpsertNodeInput,
} from '../models/fleet.models';

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export interface DagStageColumn {
  stage: number;
  tasks: MissionTask[];
}

/** Groups a mission's tasks into topological DAG stage columns (Stage 0, Stage 1, ...). */
export function computeDagStages(tasks: readonly MissionTask[]): DagStageColumn[] {
  const byId = new Map(tasks.map((t) => [t.id, t]));
  const memo = new Map<string, number>();

  const depthOf = (id: string, visiting: Set<string>): number => {
    const cached = memo.get(id);
    if (cached !== undefined) return cached;
    if (visiting.has(id)) return 0;
    visiting.add(id);
    const task = byId.get(id);
    let stage = 0;
    if (task && task.dependsOn.length > 0) {
      stage = Math.max(...task.dependsOn.map((dep) => depthOf(dep, visiting) + 1));
    } else if (task && task.depth > 0) {
      stage = task.depth;
    }
    visiting.delete(id);
    memo.set(id, stage);
    return stage;
  };

  const buckets = new Map<number, MissionTask[]>();
  for (const task of tasks) {
    const stage = depthOf(task.id, new Set());
    const list = buckets.get(stage) ?? [];
    list.push(task);
    buckets.set(stage, list);
  }
  return [...buckets.entries()]
    .sort(([a], [b]) => a - b)
    .map(([stage, stageTasks]) => ({ stage, tasks: stageTasks }));
}

const PREVIEW_OVERVIEW: FleetStateOverview = {
  nodes: [
    {
      id: 'node-local',
      name: 'Windows 主力机 (Local)',
      transportType: 'local',
      endpoint: 'http://127.0.0.1:7421/mcp',
      status: 'online',
      tags: ['windows', 'local', 'gpu'],
      maxConcurrency: 4,
      lastHeartbeatMs: Date.now(),
      lastError: null,
    },
    {
      id: 'node-mac',
      name: 'Mac Studio 构建机 (LAN)',
      transportType: 'lan',
      endpoint: 'https://192.168.1.50:7421/mcp',
      status: 'online',
      tags: ['macos', 'ios', 'arm64'],
      maxConcurrency: 3,
      lastHeartbeatMs: Date.now(),
      lastError: null,
    },
  ],
  snapshots: {
    'node-local': {
      nodeId: 'node-local',
      workspaces: [
        {
          id: 'ws-termexo',
          name: 'Termexo',
          projectPath: 'D:/devlop/Termexo',
          terminalIds: ['term-1'],
        },
      ],
      terminals: [
        {
          id: 'term-1',
          workspaceId: 'ws-termexo',
          name: 'Claude · Planner',
          agentType: 'claude',
          model: 'claude-opus-4-6',
          status: 'RUNNING',
          live: true,
          accountProfileId: 'acc-claude-pro',
        },
      ],
      settings: {
        modelProfiles: [
          {
            id: 'prof-anthropic',
            name: 'Anthropic Official',
            provider: 'Anthropic',
            isDefault: true,
            hasCredential: true,
            claudeEnabled: true,
            claudeModel: 'claude-opus-4-6',
            codexEnabled: false,
            codexModel: '',
          },
          {
            id: 'prof-openai',
            name: 'OpenAI Codex',
            provider: 'OpenAI',
            isDefault: true,
            hasCredential: true,
            claudeEnabled: false,
            claudeModel: '',
            codexEnabled: true,
            codexModel: 'gpt-5.4',
          },
        ],
        accountProfiles: [
          {
            id: 'acc-claude-pro',
            name: 'Claude Max #1',
            agentType: 'claude',
            authenticated: true,
            isDefault: true,
            isSystem: false,
          },
          {
            id: 'acc-codex-1',
            name: 'Codex Team #1',
            agentType: 'codex',
            authenticated: true,
            isDefault: true,
            isSystem: false,
          },
        ],
        mcpProfiles: [{ id: 'mcp-fleet', name: 'Termexo Fleet' }],
      },
      syncedAtMs: Date.now(),
    },
    'node-mac': {
      nodeId: 'node-mac',
      workspaces: [
        {
          id: 'ws-termexo-mac',
          name: 'Termexo (macOS)',
          projectPath: '/Users/dev/Termexo',
          terminalIds: [],
        },
      ],
      terminals: [],
      settings: { modelProfiles: [], accountProfiles: [], mcpProfiles: [] },
      syncedAtMs: Date.now(),
    },
  },
  slots: [
    {
      id: 'node-local:opencode:fast',
      nodeId: 'node-local',
      label: 'DeepSeek Flash · L1 快速档',
      agentType: 'opencode',
      accountProfileId: 'acc-oc-1',
      modelProfileId: null,
      modelName: 'deepseek-v4-flash',
      modelFamily: 'deepseek',
      supportedTiers: ['L1', 'L2'],
      costTier: 1,
      qualityTier: 2,
      maxConcurrency: 4,
      activeCount: 0,
      priority: 15,
      health: 'healthy',
      cooldownUntilMs: null,
    },
    {
      id: 'node-local:claude:prof-anthropic:acc-claude-pro',
      nodeId: 'node-local',
      label: 'Anthropic Official · Claude Max #1',
      agentType: 'claude',
      accountProfileId: 'acc-claude-pro',
      modelProfileId: 'prof-anthropic',
      modelName: 'claude-opus-4-6',
      modelFamily: 'anthropic',
      supportedTiers: ['L3', 'L4'],
      costTier: 5,
      qualityTier: 5,
      maxConcurrency: 2,
      activeCount: 1,
      priority: 20,
      health: 'healthy',
      cooldownUntilMs: null,
    },
    {
      id: 'node-mac:codex:prof-openai:acc-codex-1',
      nodeId: 'node-mac',
      label: 'Mac Studio · Codex Team #1',
      agentType: 'codex',
      accountProfileId: 'acc-codex-1',
      modelProfileId: 'prof-openai',
      modelName: 'gpt-5.4',
      modelFamily: 'openai',
      supportedTiers: ['L2', 'L3', 'L4'],
      costTier: 4,
      qualityTier: 5,
      maxConcurrency: 2,
      activeCount: 0,
      priority: 18,
      health: 'healthy',
      cooldownUntilMs: null,
    },
  ],
  missions: [
    {
      id: 'm-demo',
      title: '重构支付调度并做跨机测试与异构审查',
      goalPrompt: '实现并发安全调度、在 macOS 节点验证构建，最后由异构模型进行 L4 安全审查。',
      nodeId: 'node-local',
      workspaceId: 'ws-termexo',
      projectId: 'proj-default',
      routingMode: 'balanced',
      workflowTemplate: 'implement_verify_review',
      requiredNodeTags: [],
      guardrails: {
        maxDepth: 2,
        maxConcurrency: 4,
        maxSubagents: 12,
        isolateWorktreeByDefault: true,
      },
      fleetMcpProfileId: 'mcp-fleet',
      status: 'running',
      createdAtMs: Date.now() - 60000,
      updatedAtMs: Date.now(),
    },
  ],
  tasks: [
    {
      id: 't-impl',
      missionId: 'm-demo',
      parentTaskId: null,
      depth: 0,
      dependsOn: [],
      requiredNodeTags: ['windows'],
      role: 'worker',
      executionMode: 'task',
      preferredTerminalId: null,
      title: '[1/3 实现] 重构并发调度核心',
      description: '完成并发调度与锁顺序优化',
      acceptanceCriteria: 'Rust 单测通过',
      difficulty: 'L3',
      autoDifficulty: false,
      preferredAgent: 'claude',
      excludeModelFamily: null,
      assignedNodeId: 'node-local',
      assignedSlotId: 'node-local:claude:prof-anthropic:acc-claude-pro',
      assignedAgentType: 'claude',
      assignedModelName: 'claude-opus-4-6',
      assignedAccountProfileId: 'acc-claude-pro',
      routingReason: 'tier=L3 · mode=balanced · node=node-local · tags=[windows]',
      termexoTaskId: 'tx-1',
      termexoTerminalId: 'term-1',
      worktreePath: null,
      worktreeBranch: null,
      status: 'completed',
      retryCount: 0,
      maxRetries: 2,
      resultSummary: '已完成调度器重构并通过 24 项并发单测。',
      diffStat: '4 files changed, 182 insertions(+), 29 deletions(-)',
      lastError: null,
      parentToken: 'pt_demo_impl',
      createdAtMs: Date.now() - 60000,
      updatedAtMs: Date.now() - 30000,
    },
    {
      id: 't-sub-audit',
      missionId: 'm-demo',
      parentTaskId: 't-impl',
      depth: 1,
      dependsOn: [],
      requiredNodeTags: ['windows'],
      role: 'worker',
      executionMode: 'task',
      preferredTerminalId: null,
      title: '子 Agent · 扫描历史锁竞争热点与边界单测',
      description: '由主 Agent 通过 fleet_spawn_subagent 自动派生，在独立 Worktree 中扫描锁路径',
      acceptanceCriteria: '输出锁竞争报告与测试补丁',
      difficulty: 'L1',
      autoDifficulty: false,
      preferredAgent: 'opencode',
      excludeModelFamily: null,
      assignedNodeId: 'node-local',
      assignedSlotId: 'node-local:opencode:fast',
      assignedAgentType: 'opencode',
      assignedModelName: 'deepseek-v4-flash',
      assignedAccountProfileId: 'acc-oc-1',
      routingReason: 'tier=L1 · mode=balanced · subagent depth=1 · isolated worktree',
      termexoTaskId: 'tx-sub-1',
      termexoTerminalId: 'term-sub-1',
      worktreePath: 'D:/devlop/.termexo-fleet-worktrees/Termexo-t-sub-audit',
      worktreeBranch: 'fleet/t-sub-audit',
      status: 'completed',
      retryCount: 0,
      maxRetries: 2,
      resultSummary: '已定位 2 处嵌套读写锁热点并补齐 6 条并发边界用例。',
      diffStat: '2 files changed, 64 insertions(+)',
      lastError: null,
      parentToken: 'pt_demo_sub1',
      createdAtMs: Date.now() - 52000,
      updatedAtMs: Date.now() - 36000,
    },
    {
      id: 't-verify',
      missionId: 'm-demo',
      parentTaskId: null,
      depth: 0,
      dependsOn: ['t-impl'],
      requiredNodeTags: ['macos'],
      role: 'worker',
      executionMode: 'task',
      preferredTerminalId: null,
      title: '[2/3 测试验证] macOS 节点回归验证',
      description: '在 Mac Studio 节点验证编译与集成测试',
      acceptanceCriteria: 'cargo test 全绿',
      difficulty: 'L2',
      autoDifficulty: false,
      preferredAgent: null,
      excludeModelFamily: null,
      assignedNodeId: 'node-mac',
      assignedSlotId: 'node-mac:codex:prof-openai:acc-codex-1',
      assignedAgentType: 'codex',
      assignedModelName: 'gpt-5.4',
      assignedAccountProfileId: 'acc-codex-1',
      routingReason: 'tier=L2 · mode=balanced · node=node-mac · tags=[macos]',
      termexoTaskId: 'tx-2',
      termexoTerminalId: 'term-2',
      worktreePath: '/Users/dev/.termexo-fleet-worktrees/Termexo-t-verify',
      worktreeBranch: 'fleet/t-verify',
      status: 'running',
      retryCount: 0,
      maxRetries: 2,
      resultSummary: null,
      diffStat: null,
      lastError: null,
      parentToken: 'pt_demo_verify',
      createdAtMs: Date.now() - 25000,
      updatedAtMs: Date.now(),
    },
    {
      id: 't-review',
      missionId: 'm-demo',
      parentTaskId: null,
      depth: 0,
      dependsOn: ['t-impl', 't-verify'],
      requiredNodeTags: [],
      role: 'reviewer',
      executionMode: 'task',
      preferredTerminalId: null,
      title: '[3/3 L4 异构审查] 安全与死锁交叉审计',
      description: '自动排除上游 Anthropic 家族，由 Codex 独立复核',
      acceptanceCriteria: '无竞态或权限漏洞',
      difficulty: 'L4',
      autoDifficulty: false,
      preferredAgent: null,
      excludeModelFamily: 'anthropic',
      assignedNodeId: null,
      assignedSlotId: null,
      assignedAgentType: null,
      assignedModelName: null,
      assignedAccountProfileId: null,
      routingReason: null,
      termexoTaskId: null,
      termexoTerminalId: null,
      worktreePath: null,
      worktreeBranch: null,
      status: 'pending',
      retryCount: 0,
      maxRetries: 2,
      resultSummary: null,
      diffStat: null,
      lastError: null,
      parentToken: 'pt_demo_review',
      createdAtMs: Date.now() - 20000,
      updatedAtMs: Date.now(),
    },
  ],
  attention: [],
  executionRecords: [
    {
      id: 'rec-preview-1',
      missionId: 'm-demo',
      taskId: 't-l1-demo',
      taskTitle: '整理 README 与路由配置注释',
      nodeId: 'node-local',
      slotId: 'node-local:opencode:fast',
      agentType: 'opencode',
      modelName: 'deepseek-v4-flash',
      difficulty: 'L1',
      status: 'completed',
      startedAtMs: Date.now() - 480000,
      finishedAtMs: Date.now() - 472000,
      durationMs: 8000,
      retryCount: 0,
      escalated: false,
      estimatedCostUnits: 1,
    },
    {
      id: 'rec-preview-2',
      missionId: 'm-demo',
      taskId: 't-l2-demo',
      taskTitle: '补齐支付模块单元测试用例',
      nodeId: 'node-mac',
      slotId: 'node-mac:codex:prof-openai:acc-codex-1',
      agentType: 'codex',
      modelName: 'gpt-5.4',
      difficulty: 'L2',
      status: 'completed',
      startedAtMs: Date.now() - 320000,
      finishedAtMs: Date.now() - 294000,
      durationMs: 26000,
      retryCount: 1,
      escalated: true,
      estimatedCostUnits: 3,
    },
    {
      id: 'rec-preview-3',
      missionId: 'm-demo',
      taskId: 't-impl',
      taskTitle: '[1/3 实现] 重构并发调度核心',
      nodeId: 'node-local',
      slotId: 'node-local:claude:prof-anthropic:acc-claude-pro',
      agentType: 'claude',
      modelName: 'claude-opus-4-6',
      difficulty: 'L3',
      status: 'completed',
      startedAtMs: Date.now() - 95000,
      finishedAtMs: Date.now() - 55000,
      durationMs: 40000,
      retryCount: 0,
      escalated: false,
      estimatedCostUnits: 8,
    },
  ],
  analytics: {
    totalRuns: 3,
    completedRuns: 3,
    failedRuns: 0,
    successRatePercent: 100,
    avgDurationMs: 24666,
    escalatedRuns: 1,
    totalCostUnits: 12,
    baselineL3CostUnits: 24,
    estimatedSavingsPercent: 50,
    byTier: [
      { key: 'L1', total: 1, completed: 1, failed: 0, avgDurationMs: 8000, costUnits: 1 },
      { key: 'L2', total: 1, completed: 1, failed: 0, avgDurationMs: 26000, costUnits: 3 },
      { key: 'L3', total: 1, completed: 1, failed: 0, avgDurationMs: 40000, costUnits: 8 },
    ],
    byModel: [
      { key: 'deepseek-v4-flash', total: 1, completed: 1, failed: 0, avgDurationMs: 8000, costUnits: 1 },
      { key: 'gpt-5.4', total: 1, completed: 1, failed: 0, avgDurationMs: 26000, costUnits: 3 },
      { key: 'claude-opus-4-6', total: 1, completed: 1, failed: 0, avgDurationMs: 40000, costUnits: 8 },
    ],
    byNode: [
      { key: 'node-local', total: 2, completed: 2, failed: 0, avgDurationMs: 24000, costUnits: 9 },
      { key: 'node-mac', total: 1, completed: 1, failed: 0, avgDurationMs: 26000, costUnits: 3 },
    ],
  },
  notificationSettings: {
    webhookEnabled: false,
    webhookUrl: '',
    wechatViaTermexo: false,
    notifyOnCompleted: true,
    notifyOnFailed: true,
    notifyOnApproval: true,
    notifyOnSlotUnhealthy: true,
  },
  mcpServerUrl: 'http://127.0.0.1:7425/mcp',
  mcpServerToken: 'tf_preview_token',
};

@Injectable({ providedIn: 'root' })
export class FleetBackendService {
  readonly desktop = isTauriRuntime();
  readonly overview = signal<FleetStateOverview>(structuredClone(PREVIEW_OVERVIEW));
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);

  readonly nodes = computed(() => this.overview().nodes);
  readonly snapshots = computed(() => this.overview().snapshots);
  readonly slots = computed(() => this.overview().slots);
  readonly missions = computed(() => this.overview().missions);
  readonly tasks = computed(() => this.overview().tasks);
  readonly attention = computed(() => this.overview().attention);
  readonly executionRecords = computed(() => this.overview().executionRecords);
  readonly analytics = computed(() => this.overview().analytics);
  readonly notificationSettings = computed(() => this.overview().notificationSettings);

  private unlisten?: UnlistenFn;

  async initialize(): Promise<void> {
    if (!this.desktop) {
      return;
    }
    await this.refresh();
    if (!this.unlisten) {
      this.unlisten = await listen('fleet-state-changed', () => {
        void this.refresh();
      });
    }
  }

  destroy(): void {
    this.unlisten?.();
    this.unlisten = undefined;
  }

  async refresh(): Promise<void> {
    if (!this.desktop) return;
    try {
      const state = await invoke<FleetStateOverview>('get_fleet_overview');
      this.overview.set(state);
      this.error.set(null);
    } catch (err) {
      this.error.set(err instanceof Error ? err.message : String(err));
    }
  }

  async upsertNode(input: UpsertNodeInput): Promise<FleetNode> {
    this.busy.set(true);
    this.error.set(null);
    try {
      if (!this.desktop) {
        const node: FleetNode = {
          id: input.id ?? `node-${Date.now().toString(36)}`,
          name: input.name,
          transportType: input.transportType,
          endpoint: input.endpoint,
          allowInsecureTls: Boolean(input.allowInsecureTls),
          status: 'online',
          tags: input.tags,
          maxConcurrency: input.maxConcurrency,
          lastHeartbeatMs: Date.now(),
          lastError: null,
        };
        this.overview.update((state) => ({
          ...state,
          nodes: [...state.nodes.filter((n) => n.id !== node.id), node],
        }));
        return node;
      }
      const saved = await invoke<FleetNode>('upsert_fleet_node', { input });
      await this.refresh();
      return saved;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.error.set(msg);
      throw new Error(msg);
    } finally {
      this.busy.set(false);
    }
  }

  async removeNode(nodeId: string): Promise<void> {
    if (!this.desktop) {
      this.overview.update((state) => ({
        ...state,
        nodes: state.nodes.filter((n) => n.id !== nodeId),
        slots: state.slots.filter((s) => s.nodeId !== nodeId),
      }));
      return;
    }
    await invoke('remove_fleet_node', { nodeId });
    await this.refresh();
  }

  async syncNode(nodeId: string): Promise<void> {
    this.busy.set(true);
    try {
      if (this.desktop) {
        await invoke('sync_fleet_node', { nodeId });
        await this.refresh();
      }
    } catch (err) {
      this.error.set(err instanceof Error ? err.message : String(err));
    } finally {
      this.busy.set(false);
    }
  }

  async saveSlot(slot: AccountModelSlot): Promise<void> {
    if (!this.desktop) {
      this.overview.update((state) => ({
        ...state,
        slots: state.slots.map((s) => (s.id === slot.id ? slot : s)),
      }));
      return;
    }
    await invoke('save_account_slot', { slot });
    await this.refresh();
  }

  async toggleSlotTier(slot: AccountModelSlot, tier: DifficultyTier): Promise<void> {
    const exists = slot.supportedTiers.includes(tier);
    const nextTiers = exists
      ? slot.supportedTiers.filter((item) => item !== tier)
      : ([...slot.supportedTiers, tier] as DifficultyTier[]);
    await this.saveSlot({ ...slot, supportedTiers: nextTiers });
  }

  async resetSlotHealth(slotId: string): Promise<void> {
    if (!this.desktop) {
      this.overview.update((state) => ({
        ...state,
        slots: state.slots.map((s) =>
          s.id === slotId ? { ...s, health: 'healthy', cooldownUntilMs: null } : s,
        ),
      }));
      return;
    }
    await invoke('reset_account_slot_health', { slotId });
    await this.refresh();
  }

  async createMission(input: CreateMissionInput): Promise<[Mission, MissionTask]> {
    this.busy.set(true);
    this.error.set(null);
    try {
      if (!this.desktop) {
        const now = Date.now();
        const missionId = `m-${now.toString(36)}`;
        const mission: Mission = {
          id: missionId,
          title: input.title,
          goalPrompt: input.goalPrompt,
          nodeId: input.nodeId,
          workspaceId: input.workspaceId,
          projectId: input.projectId,
          routingMode: input.routingMode,
          workflowTemplate: input.workflowTemplate,
          requiredNodeTags: input.requiredNodeTags,
          guardrails: input.guardrails,
          fleetMcpProfileId: input.fleetMcpProfileId,
          status: input.autoDispatch ? 'running' : 'draft',
          createdAtMs: now,
          updatedAtMs: now,
        };
        const slot = this.slots()[0];
        const mkTask = (
          suffix: string,
          title: string,
          role: MissionTask['role'],
          difficulty: DifficultyTier,
          dependsOn: string[],
          running: boolean,
        ): MissionTask => ({
          id: `t-${now.toString(36)}-${suffix}`,
          missionId,
          parentTaskId: null,
          depth: 0,
          dependsOn,
          requiredNodeTags: input.requiredNodeTags,
          role,
          executionMode: input.executionMode,
          preferredTerminalId: input.preferredTerminalId,
          title,
          description: input.goalPrompt,
          acceptanceCriteria: '',
          difficulty,
          autoDifficulty: input.difficulty === null,
          preferredAgent: input.preferredAgent,
          excludeModelFamily: role === 'reviewer' ? 'anthropic' : null,
          assignedNodeId: running ? input.nodeId : null,
          assignedSlotId: running ? (slot?.id ?? null) : null,
          assignedAgentType: running ? (slot?.agentType ?? 'claude') : null,
          assignedModelName: running ? (slot?.modelName ?? 'claude-opus-4-6') : null,
          assignedAccountProfileId: running ? (slot?.accountProfileId ?? null) : null,
          routingReason: running ? `tier=${difficulty} · mode=${input.routingMode}` : null,
          termexoTaskId: null,
          termexoTerminalId: null,
          worktreePath: null,
          worktreeBranch: null,
          status: running ? 'running' : 'pending',
          retryCount: 0,
          maxRetries: 2,
          resultSummary: null,
          diffStat: null,
          lastError: null,
          parentToken: `pt_${now.toString(36)}_${suffix}`,
          createdAtMs: now,
          updatedAtMs: now,
        });

        let createdTasks: MissionTask[];
        if (input.workflowTemplate === 'implement_verify_review') {
          const t1 = mkTask(
            '1',
            `[1/3 实现] ${input.title}`,
            'worker',
            input.difficulty ?? 'L2',
            [],
            input.autoDispatch,
          );
          const t2 = mkTask('2', `[2/3 测试验证] ${input.title}`, 'worker', 'L2', [t1.id], false);
          const t3 = mkTask(
            '3',
            `[3/3 L4 异构审查] ${input.title}`,
            'reviewer',
            'L4',
            [t1.id, t2.id],
            false,
          );
          createdTasks = [t1, t2, t3];
        } else if (input.workflowTemplate === 'planner_dag') {
          createdTasks = [
            mkTask('plan', `[Planner] ${input.title}`, 'planner', 'L3', [], input.autoDispatch),
          ];
        } else {
          createdTasks = [
            mkTask('root', input.title, 'worker', input.difficulty ?? 'L2', [], input.autoDispatch),
          ];
        }

        this.overview.update((state) => ({
          ...state,
          missions: [mission, ...state.missions],
          tasks: [...state.tasks, ...createdTasks],
        }));
        return [mission, createdTasks[0]];
      }
      const created = await invoke<[Mission, MissionTask]>('create_mission', { input });
      await this.refresh();
      return created;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.error.set(msg);
      throw new Error(msg);
    } finally {
      this.busy.set(false);
    }
  }

  async dispatchTask(taskId: string): Promise<void> {
    this.busy.set(true);
    try {
      if (this.desktop) {
        await invoke('dispatch_mission_task', { taskId });
        await this.refresh();
      }
    } catch (err) {
      this.error.set(err instanceof Error ? err.message : String(err));
    } finally {
      this.busy.set(false);
    }
  }

  async retryTaskWithEscalation(taskId: string): Promise<void> {
    this.busy.set(true);
    try {
      if (!this.desktop) {
        this.overview.update((state) => ({
          ...state,
          tasks: state.tasks.map((t) => {
            if (t.id !== taskId) return t;
            const nextTier: DifficultyTier =
              t.difficulty === 'L1' ? 'L2' : t.difficulty === 'L2' ? 'L3' : t.difficulty;
            return {
              ...t,
              difficulty: nextTier,
              retryCount: t.retryCount + 1,
              status: 'running',
              lastError: null,
            };
          }),
        }));
        return;
      }
      await invoke('retry_task_with_escalation', { taskId });
      await this.refresh();
    } catch (err) {
      this.error.set(err instanceof Error ? err.message : String(err));
    } finally {
      this.busy.set(false);
    }
  }

  async cancelTask(taskId: string): Promise<void> {
    this.busy.set(true);
    try {
      if (!this.desktop) {
        this.overview.update((state) => ({
          ...state,
          tasks: state.tasks.map((t) => (t.id === taskId ? { ...t, status: 'cancelled' } : t)),
        }));
        return;
      }
      await invoke('cancel_mission_task', { taskId });
      await this.refresh();
    } catch (err) {
      this.error.set(err instanceof Error ? err.message : String(err));
    } finally {
      this.busy.set(false);
    }
  }

  async advanceMissionDag(missionId: string): Promise<void> {
    if (!this.desktop) return;
    await invoke('advance_mission_dag', { missionId });
    await this.refresh();
  }

  async replyTerminal(nodeId: string, terminalId: string, message: string): Promise<void> {
    if (!this.desktop) return;
    await invoke('reply_node_terminal', { nodeId, terminalId, message, submit: true });
    await this.refresh();
  }

  async readTerminalTail(nodeId: string, terminalId: string): Promise<string> {
    if (!this.desktop) {
      return `[Preview terminal ${terminalId}]\nTermexo Fleet connected and monitoring agent progress...`;
    }
    return invoke<string>('read_node_terminal_tail', { nodeId, terminalId });
  }

  async probeNode(
    endpoint: string,
    token: string,
    allowInsecureTls: boolean,
  ): Promise<NodeProbeResult> {
    if (!this.desktop) {
      return {
        reachable: endpoint.trim().length > 0,
        latencyMs: 14,
        workspaceCount: 2,
        terminalCount: 3,
        supportsAccountRouting: true,
        supportsMcpProfiles: true,
        error: endpoint.trim().length > 0 ? null : 'Node MCP endpoint is required',
      };
    }
    return invoke<NodeProbeResult>('probe_fleet_node', {
      endpoint,
      token,
      allowInsecureTls,
    });
  }

  async saveNotificationSettings(settings: NotificationSettings): Promise<NotificationSettings> {
    this.busy.set(true);
    try {
      if (!this.desktop) {
        this.overview.update((state) => ({
          ...state,
          notificationSettings: { ...settings },
        }));
        return settings;
      }
      const saved = await invoke<NotificationSettings>('save_notification_settings', { settings });
      await this.refresh();
      return saved;
    } finally {
      this.busy.set(false);
    }
  }

  async sendTestNotification(): Promise<string> {
    this.busy.set(true);
    try {
      if (!this.desktop) {
        const cfg = this.notificationSettings();
        if (!cfg.webhookEnabled && !cfg.wechatViaTermexo) {
          throw new Error('请先启用 Webhook 或 Termexo 微信联动中的至少一项通知通道');
        }
        return '预览模式：已模拟发送测试通知';
      }
      return await invoke<string>('send_test_notification');
    } finally {
      this.busy.set(false);
    }
  }

  buildTermexoMcpProfileSnippet(): string {
    const state = this.overview();
    return JSON.stringify(
      {
        mcpServers: {
          'termexo-fleet': {
            type: 'http',
            url: state.mcpServerUrl,
            headers: {
              Authorization: `Bearer ${state.mcpServerToken}`,
            },
          },
        },
      },
      null,
      2,
    );
  }
}
