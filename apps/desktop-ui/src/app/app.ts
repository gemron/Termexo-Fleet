import { CommonModule } from '@angular/common';
import { Component, HostListener, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';

import type {
  AccountModelSlot,
  AgentType,
  AnalyticsBucket,
  DifficultyTier,
  ExecutionMode,
  FleetNode,
  MissionTask,
  NodeProbeResult,
  NotificationSettings,
  RoutingMode,
  TransportType,
  WorkflowTemplate,
} from './core/models/fleet.models';
import { computeDagStages, FleetBackendService } from './core/services/fleet-backend.service';
import { FleetIconComponent } from './shared/fleet-icon.component';

type FleetTab = 'missions' | 'tree' | 'routing' | 'inbox' | 'nodes' | 'analytics';

interface WorkflowOption {
  id: WorkflowTemplate;
  title: string;
  badge: string;
  flow: string[];
  desc: string;
  icon: string;
}

interface DifficultyOption {
  id: 'auto' | DifficultyTier;
  label: string;
  hint: string;
  tone: string;
}

interface MissionPreset {
  label: string;
  title: string;
  goal: string;
  workflow: WorkflowTemplate;
  difficulty: 'auto' | DifficultyTier;
}

interface TerminalDrawerState {
  nodeId: string;
  terminalId: string;
  title: string;
  model: string;
  status: string;
  branch: string | null;
  text: string;
  loading: boolean;
}

interface CommandAction {
  id: string;
  title: string;
  hint: string;
  group: string;
  icon: string;
  run: () => void;
}

const MISSION_PRESETS: MissionPreset[] = [
  {
    label: '修 Bug + 回归测试',
    title: '定位并修复线上报错，补齐单测',
    goal: '定位问题根因并最小化修改，补齐对应单元测试并运行全部相关测试确认通过。',
    workflow: 'implement_verify_review',
    difficulty: 'L2',
  },
  {
    label: '架构重构 + L4 审查',
    title: '重构核心并发调度并做安全交叉审查',
    goal: '重构核心模块并消除潜在死锁/竞态，随后由不同模型家族进行 L4 安全与边界审计。',
    workflow: 'implement_verify_review',
    difficulty: 'L3',
  },
  {
    label: '多机分治并行构建',
    title: 'Planner 拆解多机并行开发与验证',
    goal: '先分析代码库输出并行 DAG 计划，按标签分派到不同机器并行实现并汇总验证结果。',
    workflow: 'planner_dag',
    difficulty: 'auto',
  },
  {
    label: '轻量文档与巡检',
    title: '快速整理 README 与代码注释',
    goal: '检查近期改动，补齐清晰的使用文档与关键接口注释，保持原有逻辑不变。',
    workflow: 'direct',
    difficulty: 'L1',
  },
];

const WORKFLOW_OPTIONS: WorkflowOption[] = [
  {
    id: 'implement_verify_review',
    title: '三段式质量流水线',
    badge: '推荐',
    flow: ['实现', '测试', 'L4 异构审'],
    desc: '写完自动测试，最后强制用不同模型家族独立复核',
    icon: 'workflow',
  },
  {
    id: 'planner_dag',
    title: '智能规划官 DAG',
    badge: '分治',
    flow: ['Planner 拆解', '多机并行', '汇总'],
    desc: '先在 Termexo 读代码出 DAG 计划，再自动跨机并行执行',
    icon: 'sparkles',
  },
  {
    id: 'direct',
    title: '单核蜂群自主派生',
    badge: '灵活',
    flow: ['父 Agent', '自主开子 Agent'],
    desc: '单主任务启动，由模型按需自主雇佣子 Agent 并行探索',
    icon: 'tree',
  },
];

const DIFFICULTY_OPTIONS: DifficultyOption[] = [
  { id: 'auto', label: 'Auto 智能', hint: '按描述自动评估', tone: 'auto' },
  { id: 'L1', label: 'L1 闪电', hint: '轻量快速 · 省钱', tone: 'l1' },
  { id: 'L2', label: 'L2 标准', hint: '常规功能与单测', tone: 'l2' },
  { id: 'L3', label: 'L3 攻坚', hint: '架构重构 · 顶级推理', tone: 'l3' },
  { id: 'L4', label: 'L4 异构审', hint: '跨模型家族交叉审查', tone: 'l4' },
];

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [CommonModule, FormsModule, FleetIconComponent],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit, OnDestroy {
  protected readonly backend = inject(FleetBackendService);
  protected readonly activeTab = signal<FleetTab>('missions');
  protected readonly toast = signal<string | null>(null);
  protected readonly terminalDrawer = signal<TerminalDrawerState | null>(null);
  protected readonly drawerReplyDraft = signal('');
  protected readonly advancedComposerOpen = signal(false);
  protected readonly nodeModalOpen = signal(false);
  protected readonly mcpDrawerOpen = signal(false);
  protected readonly commandPaletteOpen = signal(false);
  protected readonly commandQuery = signal('');
  protected readonly missionFilter = signal<'all' | 'active' | 'completed' | 'failed'>('all');

  protected readonly workflowOptions = WORKFLOW_OPTIONS;
  protected readonly difficultyOptions = DIFFICULTY_OPTIONS;
  protected readonly missionPresets = MISSION_PRESETS;
  protected readonly allTiers: DifficultyTier[] = ['L1', 'L2', 'L3', 'L4'];
  protected readonly quickReplies = [
    { label: '同意 (y)', text: 'y' },
    { label: '拒绝 (n)', text: 'n' },
    { label: '继续执行', text: '继续执行，完成后汇报改动摘要。' },
  ];

  // Add / edit Termexo node form
  protected readonly editingNodeId = signal<string | undefined>(undefined);
  protected readonly nodeName = signal('Local Termexo');
  protected readonly nodeTransport = signal<TransportType>('local');
  protected readonly nodeEndpoint = signal('http://127.0.0.1:7421/mcp');
  protected readonly nodeToken = signal('');
  protected readonly nodeTags = signal('local, windows');
  protected readonly nodeMaxConcurrency = signal(4);
  protected readonly nodeAllowInsecureTls = signal(false);
  protected readonly nodeProbing = signal(false);
  protected readonly nodeProbeResult = signal<NodeProbeResult | null>(null);

  // Notification settings form
  protected readonly notifyWebhookEnabled = signal(false);
  protected readonly notifyWebhookUrl = signal('');
  protected readonly notifyWechatViaTermexo = signal(false);
  protected readonly notifyOnCompleted = signal(true);
  protected readonly notifyOnFailed = signal(true);
  protected readonly notifyOnApproval = signal(true);
  protected readonly notifyOnSlotUnhealthy = signal(true);
  protected readonly notifyTestFeedback = signal<string | null>(null);

  // Create Mission form
  protected readonly missionTitle = signal('');
  protected readonly missionGoal = signal('');
  protected readonly selectedNodeId = signal('');
  protected readonly selectedWorkspaceId = signal('');
  protected readonly selectedWorkflow = signal<WorkflowTemplate>('implement_verify_review');
  protected readonly selectedExecutionMode = signal<ExecutionMode>('task');
  protected readonly selectedRoutingMode = signal<RoutingMode>('balanced');
  protected readonly selectedDifficulty = signal<'auto' | DifficultyTier>('auto');
  protected readonly selectedAgent = signal<'' | AgentType>('');
  protected readonly requiredTagsInput = signal('');
  protected readonly selectedMcpProfileId = signal('');
  protected readonly maxDepth = signal(2);
  protected readonly maxConcurrency = signal(4);
  protected readonly maxSubagents = signal(12);
  protected readonly isolateWorktree = signal(true);

  // Interactive Routing Simulator
  protected readonly simulatorPrompt = signal('重构支付模块的并发锁机制并进行安全审查');
  protected readonly simulatorMode = signal<RoutingMode>('balanced');

  protected readonly replyDrafts = signal<Record<string, string>>({});

  protected readonly resolvedNodeId = computed(
    () => this.selectedNodeId() || this.backend.nodes()[0]?.id || '',
  );

  protected readonly activeNodeSnapshot = computed(
    () => this.backend.snapshots()[this.resolvedNodeId()] ?? null,
  );

  protected readonly resolvedWorkspaceId = computed(
    () => this.selectedWorkspaceId() || this.activeNodeSnapshot()?.workspaces[0]?.id || '',
  );

  protected readonly availableMcpProfiles = computed(
    () => this.activeNodeSnapshot()?.settings.mcpProfiles ?? [],
  );

  protected readonly onlineNodeCount = computed(
    () => this.backend.nodes().filter((n) => n.status === 'online').length,
  );

  protected readonly totalNodeCapacity = computed(() =>
    this.backend
      .nodes()
      .filter((n) => n.status === 'online')
      .reduce((sum, n) => sum + n.maxConcurrency, 0),
  );

  protected readonly healthySlotRate = computed(() => {
    const slots = this.backend.slots();
    if (!slots.length) return 100;
    const healthy = slots.filter((s) => s.health === 'healthy').length;
    return Math.round((healthy / slots.length) * 100);
  });

  protected readonly runningTaskCount = computed(
    () =>
      this.backend
        .tasks()
        .filter(
          (t) =>
            t.status === 'running' ||
            t.status === 'waiting_input' ||
            t.status === 'waiting_approval',
        ).length,
  );

  protected readonly needsOnboarding = computed(
    () => this.backend.desktop && this.onlineNodeCount() === 0,
  );

  protected readonly missionsWithDag = computed(() => {
    const tasks = this.backend.tasks();
    return this.backend.missions().map((mission) => {
      const missionTasks = tasks.filter((t) => t.missionId === mission.id);
      const completed = missionTasks.filter((t) => t.status === 'completed').length;
      const running = missionTasks.filter((t) => t.status === 'running').length;
      const progress = missionTasks.length
        ? Math.round((completed / missionTasks.length) * 100)
        : 0;
      return {
        mission,
        stages: computeDagStages(missionTasks),
        tasks: missionTasks,
        completedCount: completed,
        runningCount: running,
        progress,
      };
    });
  });

  protected readonly filteredMissionsWithDag = computed(() => {
    const filter = this.missionFilter();
    const list = this.missionsWithDag();
    if (filter === 'all') return list;
    if (filter === 'active') {
      return list.filter((g) => g.mission.status === 'running' || g.mission.status === 'draft');
    }
    return list.filter((g) => g.mission.status === filter);
  });

  /** Live routing forecast shown inside the mission composer before dispatch. */
  protected readonly composerRoutePreview = computed(() => {
    const chosen = this.selectedDifficulty();
    const combined = `${this.missionTitle()} ${this.missionGoal()}`.trim().toLowerCase();
    let tier: DifficultyTier;
    let reason: string;
    if (chosen !== 'auto') {
      tier = chosen;
      reason = `已锁定 ${tier} 档位`;
    } else if (/(review|audit|security|审查|评审|安全|交叉验证)/.test(combined)) {
      tier = 'L4';
      reason = '语义命中安全/审查，自动升档 L4';
    } else if (
      /(architect|refactor|concurrency|deadlock|migration|performance|架构|重构|并发|死锁|性能|底层)/.test(
        combined,
      ) ||
      combined.length > 180
    ) {
      tier = 'L3';
      reason = '复杂架构/并发特征，自动匹配 L3';
    } else if (/(typo|readme|comment|format|lint|文案|注释|格式化|查看|列出|文档)/.test(combined)) {
      tier = 'L1';
      reason = '轻量改动特征，自动省钱匹配 L1';
    } else {
      tier = 'L2';
      reason = '标准研发任务，自动匹配 L2 均衡档';
    }
    const winner = this.pickSimulatedSlot(tier, this.selectedRoutingMode());
    return { tier, reason, winner };
  });

  protected readonly filteredCommands = computed<CommandAction[]>(() => {
    const q = this.commandQuery().trim().toLowerCase();
    const base: CommandAction[] = [
      {
        id: 'go-missions',
        title: '任务指挥甲板',
        hint: '下发新任务与查看实时 DAG 流水线',
        group: '导航视图',
        icon: 'workflow',
        run: () => this.activeTab.set('missions'),
      },
      {
        id: 'go-tree',
        title: '子 Agent 蜂群树',
        hint: '查看父子派生树与护栏仪表盘',
        group: '导航视图',
        icon: 'tree',
        run: () => this.activeTab.set('tree'),
      },
      {
        id: 'go-routing',
        title: '难度天梯与账号池',
        hint: '测试 L1–L4 路由策略与调整槽位档位',
        group: '导航视图',
        icon: 'route',
        run: () => this.activeTab.set('routing'),
      },
      {
        id: 'go-inbox',
        title: '全局待办收件箱',
        hint: '集中处理等待审批与等待输入的终端',
        group: '导航视图',
        icon: 'inbox',
        run: () => this.activeTab.set('inbox'),
      },
      {
        id: 'go-nodes',
        title: '多机舰队拓扑',
        hint: '管理本机、局域网/VPN 与 Relay 节点',
        group: '导航视图',
        icon: 'radar',
        run: () => this.activeTab.set('nodes'),
      },
      {
        id: 'go-analytics',
        title: '效能账本与告警通知',
        hint: '查看节省比例、模型排行与 Webhook / 微信配置',
        group: '导航视图',
        icon: 'chart',
        run: () => this.openAnalyticsTab(),
      },
      {
        id: 'act-connect',
        title: '连接新 Termexo 节点',
        hint: '打开节点连接与连通性探测弹窗',
        group: '快捷操作',
        icon: 'plus',
        run: () => this.openNewNodeModal(),
      },
      {
        id: 'act-sync',
        title: '立即同步全部舰队节点',
        hint: '刷新工作区、终端与账号状态',
        group: '快捷操作',
        icon: 'refresh',
        run: () => {
          void this.backend.refresh();
          this.notify('已触发全舰队状态同步');
        },
      },
      {
        id: 'act-mcp',
        title: '复制反向 MCP 接入 JSON',
        hint: '用于给 Termexo 配置自动子 Agent 能力',
        group: '快捷操作',
        icon: 'copy',
        run: () => void this.copyFleetMcpConfig(),
      },
    ];
    if (!q) return base;
    return base.filter(
      (item) =>
        item.title.toLowerCase().includes(q) ||
        item.hint.toLowerCase().includes(q) ||
        item.group.toLowerCase().includes(q),
    );
  });

  protected readonly taskTreeByMission = computed(() => {
    const tasks = this.backend.tasks();
    return this.backend.missions().map((mission) => {
      const missionTasks = tasks.filter((t) => t.missionId === mission.id);
      const ordered: MissionTask[] = [];
      const visit = (parentId: string | null) => {
        for (const item of missionTasks.filter((t) => t.parentTaskId === parentId)) {
          ordered.push(item);
          visit(item.id);
        }
      };
      visit(null);
      const maxObservedDepth = missionTasks.reduce((max, t) => Math.max(max, t.depth), 0);
      const subagentTasks = missionTasks.filter((t) => t.parentTaskId !== null);
      const activeSubagents = subagentTasks.filter((t) =>
        ['pending', 'queued', 'running', 'waiting_input', 'waiting_approval'].includes(t.status),
      ).length;
      return {
        mission,
        tasks: ordered,
        maxObservedDepth,
        activeSubagents,
        totalSubagents: subagentTasks.length,
      };
    });
  });

  /** Live L1-L4 Tier Ladder showing which exact node/slot wins each difficulty right now. */
  protected readonly routingLadder = computed(() => {
    const mode = this.simulatorMode();
    return this.allTiers.map((tier) => ({
      tier,
      meta: this.difficultyOptions.find((d) => d.id === tier)!,
      winner: this.pickSimulatedSlot(tier, mode),
    }));
  });

  /** Interactive simulation for any user-typed prompt in the Routing tab. */
  protected readonly simulatedRoute = computed(() => {
    const text = this.simulatorPrompt().trim().toLowerCase();
    let tier: DifficultyTier = 'L2';
    let reason = '常规研发任务，匹配标准均衡档';
    if (/(review|audit|security|审查|评审|安全|交叉验证)/.test(text)) {
      tier = 'L4';
      reason = '命中安全/审查语义，启用 L4 跨模型家族异构审查';
    } else if (/(architect|refactor|concurrency|deadlock|migration|performance|架构|重构|并发|死锁|性能|底层)/.test(text) || text.length > 200) {
      tier = 'L3';
      reason = '命中复杂工程/重构/并发特征，升级至 L3 顶级推理档';
    } else if (/(typo|readme|comment|format|lint|文案|注释|格式化|查看|列出)/.test(text)) {
      tier = 'L1';
      reason = '轻量查询或机械改动，分配至 L1 低成本闪电档';
    }
    const winner = this.pickSimulatedSlot(tier, this.simulatorMode());
    return { tier, reason, winner };
  });

  ngOnInit(): void {
    void this.backend.initialize().then(() => {
      this.syncNotificationFormFromState();
    });
  }

  ngOnDestroy(): void {
    this.backend.destroy();
  }

  protected openAnalyticsTab(): void {
    this.syncNotificationFormFromState();
    this.activeTab.set('analytics');
  }

  protected syncNotificationFormFromState(): void {
    const cfg = this.backend.notificationSettings();
    this.notifyWebhookEnabled.set(cfg.webhookEnabled);
    this.notifyWebhookUrl.set(cfg.webhookUrl);
    this.notifyWechatViaTermexo.set(cfg.wechatViaTermexo);
    this.notifyOnCompleted.set(cfg.notifyOnCompleted);
    this.notifyOnFailed.set(cfg.notifyOnFailed);
    this.notifyOnApproval.set(cfg.notifyOnApproval);
    this.notifyOnSlotUnhealthy.set(cfg.notifyOnSlotUnhealthy);
  }

  protected async saveNotificationConfig(): Promise<void> {
    const settings: NotificationSettings = {
      webhookEnabled: this.notifyWebhookEnabled(),
      webhookUrl: this.notifyWebhookUrl().trim(),
      wechatViaTermexo: this.notifyWechatViaTermexo(),
      notifyOnCompleted: this.notifyOnCompleted(),
      notifyOnFailed: this.notifyOnFailed(),
      notifyOnApproval: this.notifyOnApproval(),
      notifyOnSlotUnhealthy: this.notifyOnSlotUnhealthy(),
    };
    await this.backend.saveNotificationSettings(settings);
    this.notify('告警与通知配置已保存');
  }

  protected async triggerTestNotification(): Promise<void> {
    await this.saveNotificationConfig();
    try {
      const msg = await this.backend.sendTestNotification();
      this.notifyTestFeedback.set(msg);
      this.notify(msg);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.notifyTestFeedback.set(`发送失败：${msg}`);
      this.notify(msg);
    }
  }

  protected maxBucketTotal(buckets: readonly AnalyticsBucket[]): number {
    return buckets.reduce((max, b) => Math.max(max, b.total), 1);
  }

  protected formatDuration(ms: number): string {
    if (!ms || ms < 1000) return `${Math.max(0, Math.round(ms))} ms`;
    const secs = ms / 1000;
    if (secs < 60) return `${secs.toFixed(1)} s`;
    const mins = Math.floor(secs / 60);
    const rem = Math.round(secs % 60);
    return `${mins}m ${rem}s`;
  }

  protected openNewNodeModal(): void {
    this.resetNodeForm();
    this.nodeModalOpen.set(true);
  }

  protected editNode(node: FleetNode): void {
    this.editingNodeId.set(node.id);
    this.nodeName.set(node.name);
    this.nodeTransport.set(node.transportType);
    this.nodeEndpoint.set(node.endpoint);
    this.nodeTags.set(node.tags.join(', '));
    this.nodeMaxConcurrency.set(node.maxConcurrency);
    this.nodeAllowInsecureTls.set(Boolean(node.allowInsecureTls));
    this.nodeToken.set('');
    this.nodeProbeResult.set(null);
    this.nodeModalOpen.set(true);
  }

  protected resetNodeForm(): void {
    this.editingNodeId.set(undefined);
    this.nodeName.set('Local Termexo');
    this.nodeTransport.set('local');
    this.nodeEndpoint.set('http://127.0.0.1:7421/mcp');
    this.nodeTags.set('local, windows');
    this.nodeMaxConcurrency.set(4);
    this.nodeAllowInsecureTls.set(false);
    this.nodeToken.set('');
    this.nodeProbeResult.set(null);
  }

  protected async testNodeConnection(): Promise<void> {
    this.nodeProbing.set(true);
    try {
      const result = await this.backend.probeNode(
        this.nodeEndpoint(),
        this.nodeToken(),
        this.nodeAllowInsecureTls(),
      );
      this.nodeProbeResult.set(result);
    } catch (err) {
      this.nodeProbeResult.set({
        reachable: false,
        latencyMs: 0,
        workspaceCount: 0,
        terminalCount: 0,
        supportsAccountRouting: false,
        supportsMcpProfiles: false,
        error: err instanceof Error ? err.message : String(err),
      });
    } finally {
      this.nodeProbing.set(false);
    }
  }

  protected async connectNode(): Promise<void> {
    const tags = this.nodeTags()
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    await this.backend.upsertNode({
      id: this.editingNodeId(),
      name: this.nodeName(),
      transportType: this.nodeTransport(),
      endpoint: this.nodeEndpoint(),
      token: this.nodeToken() || undefined,
      tags,
      maxConcurrency: Number(this.nodeMaxConcurrency()) || 4,
      allowInsecureTls: this.nodeAllowInsecureTls(),
    });
    this.nodeModalOpen.set(false);
    this.resetNodeForm();
    this.notify('Termexo 节点已连接并完成能力同步');
  }

  protected nodeActiveCount(nodeId: string): number {
    return this.backend
      .slots()
      .filter((s) => s.nodeId === nodeId)
      .reduce((sum, s) => sum + s.activeCount, 0);
  }

  protected nodeLoadPercent(node: FleetNode): number {
    const cap = Math.max(1, node.maxConcurrency);
    return Math.min(100, Math.round((this.nodeActiveCount(node.id) / cap) * 100));
  }

  protected nodeOsTag(node: FleetNode): { label: string; tone: string } {
    const joined = [...node.tags, node.name].join(' ').toLowerCase();
    if (joined.includes('mac') || joined.includes('ios')) {
      return { label: 'macOS', tone: 'mac' };
    }
    if (joined.includes('linux') || joined.includes('ubuntu') || joined.includes('ci')) {
      return { label: 'Linux', tone: 'linux' };
    }
    return { label: 'Windows', tone: 'win' };
  }

  protected agentBadge(agent: AgentType | string | null): { short: string; name: string; tone: string } {
    switch (agent) {
      case 'claude':
        return { short: 'CL', name: 'Claude Code', tone: 'claude' };
      case 'codex':
        return { short: 'CX', name: 'Codex CLI', tone: 'codex' };
      case 'opencode':
        return { short: 'OC', name: 'OpenCode', tone: 'opencode' };
      case 'pi':
        return { short: 'PI', name: 'Pi Agent', tone: 'pi' };
      case 'grok':
        return { short: 'GK', name: 'Grok Build', tone: 'grok' };
      case 'antigravity':
        return { short: 'AG', name: 'Antigravity', tone: 'antigravity' };
      default:
        return { short: 'AI', name: '待路由', tone: 'neutral' };
    }
  }

  protected statusLabel(status: string): string {
    const map: Record<string, string> = {
      pending: '等待依赖',
      queued: '排队中',
      running: '执行中',
      waiting_input: '等待输入',
      waiting_approval: '等待审批',
      completed: '已完成',
      failed: '执行失败',
      cancelled: '已取消',
      draft: '草稿',
      paused: '已暂停',
    };
    return map[status] ?? status;
  }

  protected slotsRange(count: number): number[] {
    return Array.from({ length: Math.max(0, count) }, (_, i) => i);
  }

  protected percent(used: number, max: number): number {
    return Math.min(100, Math.round((used / Math.max(1, max)) * 100));
  }

  @HostListener('window:keydown', ['$event'])
  protected handleGlobalKeydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      this.commandQuery.set('');
      this.commandPaletteOpen.update((open) => !open);
      return;
    }
    if (event.key === 'Escape') {
      if (this.commandPaletteOpen()) this.commandPaletteOpen.set(false);
      else if (this.terminalDrawer()) this.terminalDrawer.set(null);
      else if (this.nodeModalOpen()) this.nodeModalOpen.set(false);
      else if (this.mcpDrawerOpen()) this.mcpDrawerOpen.set(false);
    }
  }

  protected runCommandAction(action: CommandAction): void {
    this.commandPaletteOpen.set(false);
    this.commandQuery.set('');
    action.run();
  }

  protected applyMissionPreset(preset: MissionPreset): void {
    this.missionTitle.set(preset.title);
    this.missionGoal.set(preset.goal);
    this.selectedWorkflow.set(preset.workflow);
    this.selectedDifficulty.set(preset.difficulty);
  }

  protected handleGoalKeydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
      event.preventDefault();
      void this.launchMission();
    }
  }

  protected applyNodePreset(kind: 'local' | 'lan' | 'tailscale'): void {
    this.nodeProbeResult.set(null);
    if (kind === 'local') {
      this.nodeName.set('本机 Termexo');
      this.nodeTransport.set('local');
      this.nodeEndpoint.set('http://127.0.0.1:7421/mcp');
      this.nodeTags.set('local, windows');
      this.nodeAllowInsecureTls.set(false);
    } else if (kind === 'lan') {
      this.nodeName.set('局域网构建机');
      this.nodeTransport.set('lan');
      this.nodeEndpoint.set('http://192.168.1.50:7421/mcp');
      this.nodeTags.set('lan, macos');
      this.nodeAllowInsecureTls.set(true);
    } else {
      this.nodeName.set('Tailscale 远程节点');
      this.nodeTransport.set('lan');
      this.nodeEndpoint.set('http://100.64.0.10:7421/mcp');
      this.nodeTags.set('vpn, linux');
      this.nodeAllowInsecureTls.set(true);
    }
  }

  protected async launchMission(): Promise<void> {
    const goal = this.missionGoal().trim();
    const rawTitle = this.missionTitle().trim();
    const title = rawTitle || goal.split(/\r?\n/)[0].slice(0, 28);
    if (!title || !goal) {
      this.notify('请输入任务目标描述（按 Ctrl+Enter 可直接下发）');
      return;
    }
    const nodeId = this.resolvedNodeId();
    const workspaceId = this.resolvedWorkspaceId();
    if (!nodeId || !workspaceId) {
      this.openNewNodeModal();
      this.notify('请先连接至少一台本机或远程 Termexo');
      return;
    }
    const mcpProfile =
      this.selectedMcpProfileId() ||
      this.availableMcpProfiles().find((p) => p.name.toLowerCase().includes('fleet'))?.id ||
      null;
    const requiredNodeTags = this.requiredTagsInput()
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    await this.backend.createMission({
      title,
      goalPrompt: goal,
      nodeId,
      workspaceId,
      projectId: '',
      routingMode: this.selectedRoutingMode(),
      workflowTemplate: this.selectedWorkflow(),
      requiredNodeTags,
      executionMode: this.selectedExecutionMode(),
      preferredTerminalId: null,
      difficulty:
        this.selectedDifficulty() === 'auto'
          ? null
          : (this.selectedDifficulty() as DifficultyTier),
      preferredAgent: this.selectedAgent() ? (this.selectedAgent() as AgentType) : null,
      guardrails: {
        maxDepth: Number(this.maxDepth()) || 2,
        maxConcurrency: Number(this.maxConcurrency()) || 4,
        maxSubagents: Number(this.maxSubagents()) || 12,
        isolateWorktreeByDefault: this.isolateWorktree(),
      },
      fleetMcpProfileId: mcpProfile,
      autoDispatch: true,
    });
    this.missionTitle.set('');
    this.missionGoal.set('');
    this.notify('任务已按难度路由并下发至 Termexo 执行');
  }

  protected async copyFleetMcpConfig(): Promise<void> {
    const snippet = this.backend.buildTermexoMcpProfileSnippet();
    try {
      await navigator.clipboard.writeText(snippet);
      this.notify('已复制反向 MCP JSON，粘贴到 Termexo「设置 → MCP Profile」即可');
    } catch {
      this.notify('请手动复制配置内容');
    }
  }

  protected async inspectTerminal(task: MissionTask): Promise<void> {
    if (!task.assignedNodeId || !task.termexoTerminalId) return;
    const nodeId = task.assignedNodeId;
    const terminalId = task.termexoTerminalId;
    this.drawerReplyDraft.set('');
    this.terminalDrawer.set({
      nodeId,
      terminalId,
      title: task.title,
      model: task.assignedModelName || this.agentBadge(task.assignedAgentType).name,
      status: task.status,
      branch: task.worktreeBranch,
      text: '正在拉取 Termexo 实时终端输出...',
      loading: true,
    });
    try {
      const text = await this.backend.readTerminalTail(nodeId, terminalId);
      this.terminalDrawer.update((cur) => (cur ? { ...cur, text, loading: false } : null));
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.terminalDrawer.update((cur) => (cur ? { ...cur, text: `读取失败：${msg}`, loading: false } : null));
    }
  }

  protected async refreshTerminalDrawer(): Promise<void> {
    const cur = this.terminalDrawer();
    if (!cur) return;
    this.terminalDrawer.set({ ...cur, loading: true });
    try {
      const text = await this.backend.readTerminalTail(cur.nodeId, cur.terminalId);
      this.terminalDrawer.update((state) => (state ? { ...state, text, loading: false } : null));
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.terminalDrawer.update((state) => (state ? { ...state, text: `读取失败：${msg}`, loading: false } : null));
    }
  }

  protected async sendDrawerReply(overrideText?: string): Promise<void> {
    const cur = this.terminalDrawer();
    if (!cur) return;
    const text = (overrideText ?? this.drawerReplyDraft()).trim();
    if (!text) return;
    await this.backend.replyTerminal(cur.nodeId, cur.terminalId, text);
    if (!overrideText) this.drawerReplyDraft.set('');
    this.notify(`已发送「${text}」至 Termexo 终端`);
    await this.refreshTerminalDrawer();
  }

  protected async nudgeWaitingTask(task: MissionTask): Promise<void> {
    if (!task.assignedNodeId || !task.termexoTerminalId) return;
    await this.backend.replyTerminal(
      task.assignedNodeId,
      task.termexoTerminalId,
      '继续执行当前任务，完成后输出改动摘要。',
    );
    this.notify(`已向「${task.title}」发送继续指令`);
  }

  protected updateReplyDraft(id: string, value: string): void {
    this.replyDrafts.update((drafts) => ({ ...drafts, [id]: value }));
  }

  protected async sendQuickReply(
    attentionId: string,
    nodeId: string,
    terminalId: string | null,
    text: string,
  ): Promise<void> {
    if (!terminalId) return;
    await this.backend.replyTerminal(nodeId, terminalId, text);
    this.updateReplyDraft(attentionId, '');
    this.notify(`已发送「${text}」到 Termexo 终端`);
  }

  protected async sendAttentionReply(
    attentionId: string,
    nodeId: string,
    terminalId: string | null,
  ): Promise<void> {
    const text = (this.replyDrafts()[attentionId] ?? '').trim();
    if (!text) return;
    await this.sendQuickReply(attentionId, nodeId, terminalId, text);
  }

  protected tierToneClass(tier: DifficultyTier): string {
    return `tier-${tier.toLowerCase()}`;
  }

  private pickSimulatedSlot(tier: DifficultyTier, mode: RoutingMode): AccountModelSlot | null {
    const healthy = this.backend
      .slots()
      .filter((s) => s.health === 'healthy' && s.activeCount < Math.max(1, s.maxConcurrency));
    if (!healthy.length) return null;
    const exact = healthy.filter((s) => !s.supportedTiers.length || s.supportedTiers.includes(tier));
    const pool = exact.length ? exact : healthy;
    const score = (slot: AccountModelSlot): number => {
      const modeScore =
        mode === 'cost'
          ? (6 - Math.min(5, slot.costTier)) * 120 + slot.qualityTier * 20
          : mode === 'quality'
            ? slot.qualityTier * 140 - slot.costTier * 10
            : 150 - Math.abs(slot.qualityTier - (tier === 'L1' ? 2 : tier === 'L2' ? 3 : 5)) * 35 + (6 - slot.costTier) * 30;
      return modeScore + slot.priority * 10 - slot.activeCount * 60;
    };
    return [...pool].sort((a, b) => score(b) - score(a))[0] ?? null;
  }

  private notify(message: string): void {
    this.toast.set(message);
    setTimeout(() => {
      if (this.toast() === message) {
        this.toast.set(null);
      }
    }, 3600);
  }
}
