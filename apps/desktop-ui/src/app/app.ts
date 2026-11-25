import { CommonModule } from '@angular/common';
import {
  Component,
  ElementRef,
  HostListener,
  OnDestroy,
  OnInit,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { UnlistenFn } from '@tauri-apps/api/event';

import type {
  AccountModelSlot,
  AgentType,
  AnalyticsBucket,
  AttentionItem,
  AttentionKind,
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
  shortTitle: string;
  badge: string;
  flow: string[];
  desc: string;
  icon: string;
}

interface DifficultyOption {
  id: 'auto' | DifficultyTier;
  label: string;
  shortLabel: string;
  hint: string;
  tone: string;
  costHint: string;
}

interface MissionPreset {
  label: string;
  title: string;
  goal: string;
  workflow: WorkflowTemplate;
  difficulty: 'auto' | DifficultyTier;
  tierTag: string;
}

interface SimulatorPreset {
  label: string;
  tier: DifficultyTier;
  prompt: string;
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
  updatedAtMs: number;
}

interface CommandAction {
  id: string;
  title: string;
  hint: string;
  group: string;
  icon: string;
  shortcut?: string;
  run: () => void;
}

const MISSION_PRESETS: MissionPreset[] = [
  {
    label: '修 Bug + 回归测试',
    title: '定位并修复线上报错，补齐单测',
    goal: '定位问题根因并最小化修改，补齐对应单元测试并运行全部相关测试确认通过。',
    workflow: 'implement_verify_review',
    difficulty: 'L2',
    tierTag: 'L2',
  },
  {
    label: '架构重构 + L4 审查',
    title: '重构核心并发调度并做安全交叉审查',
    goal: '重构核心模块并消除潜在死锁/竞态，随后由不同模型家族进行 L4 安全与边界审计。',
    workflow: 'implement_verify_review',
    difficulty: 'L3',
    tierTag: 'L3',
  },
  {
    label: '多机分治并行构建',
    title: 'Planner 拆解多机并行开发与验证',
    goal: '先分析代码库输出并行 DAG 计划，按标签分派到不同机器并行实现并汇总验证结果。',
    workflow: 'planner_dag',
    difficulty: 'auto',
    tierTag: 'DAG',
  },
  {
    label: '轻量文档与巡检',
    title: '快速整理 README 与代码注释',
    goal: '检查近期改动，补齐清晰的使用文档与关键接口注释，保持原有逻辑不变。',
    workflow: 'direct',
    difficulty: 'L1',
    tierTag: 'L1',
  },
];

const SIMULATOR_PRESETS: SimulatorPreset[] = [
  { label: '修文档错别字', tier: 'L1', prompt: '整理 README 文档和代码注释，顺手检查 lint 格式' },
  { label: '补齐单元测试', tier: 'L2', prompt: '给用户登录与会话续期接口补齐单元测试和边界用例' },
  { label: '重构并发调度', tier: 'L3', prompt: '重构任务队列的异步并发调度与锁顺序，优化吞吐性能' },
  { label: '安全交叉审查', tier: 'L4', prompt: '对支付核心模块进行安全审查与权限越界交叉验证' },
];

const WORKFLOW_OPTIONS: WorkflowOption[] = [
  {
    id: 'implement_verify_review',
    title: '三段式质量流水线',
    shortTitle: '三段式流水线',
    badge: '推荐',
    flow: ['实现', '测试验证', 'L4 异构审'],
    desc: '写完自动测试，最后强制用不同模型家族独立复核',
    icon: 'workflow',
  },
  {
    id: 'planner_dag',
    title: '智能规划官 DAG',
    shortTitle: '规划官 DAG',
    badge: '分治',
    flow: ['Planner 拆解', '多机并行', '汇总'],
    desc: '先在 Termexo 读代码出 DAG 计划，再自动跨机并行执行',
    icon: 'sparkles',
  },
  {
    id: 'direct',
    title: '单核蜂群自主派生',
    shortTitle: '自主蜂群',
    badge: '灵活',
    flow: ['父 Agent', '自主开子 Agent'],
    desc: '单主任务启动，由模型按需自主雇佣子 Agent 并行探索',
    icon: 'tree',
  },
];

const DIFFICULTY_OPTIONS: DifficultyOption[] = [
  { id: 'auto', label: 'Auto 智能', shortLabel: 'Auto', hint: '按语义自动评估最优档位', tone: 'auto', costHint: '自适应' },
  { id: 'L1', label: 'L1 闪电', shortLabel: 'L1', hint: '轻量快速 · 低成本槽位', tone: 'l1', costHint: '1 u' },
  { id: 'L2', label: 'L2 标准', shortLabel: 'L2', hint: '常规功能开发与单测', tone: 'l2', costHint: '3 u' },
  { id: 'L3', label: 'L3 攻坚', shortLabel: 'L3', hint: '架构重构 · 顶级推理', tone: 'l3', costHint: '8 u' },
  { id: 'L4', label: 'L4 异构审', shortLabel: 'L4', hint: '跨模型家族交叉审查', tone: 'l4', costHint: '8 u' },
];

const VIEW_META: Record<FleetTab, { title: string; subtitle: string; icon: string }> = {
  missions: {
    title: '任务指挥甲板',
    subtitle: '一句话下发任务，自动按难度路由并在真实 Termexo 终端中编排执行',
    icon: 'workflow',
  },
  tree: {
    title: '子 Agent 蜂群树',
    subtitle: '实时洞察父子派生拓扑、嵌套深度、并发预算与 Git Worktree 隔离',
    icon: 'tree',
  },
  routing: {
    title: '难度天梯与账号池',
    subtitle: '可视化 L1–L4 实时命中策略，点击槽位档位即时调整多机模型分工',
    icon: 'route',
  },
  inbox: {
    title: '全局待办收件箱',
    subtitle: '集中处理所有 Termexo 节点上的审批阻塞、输入等待与限流异常',
    icon: 'inbox',
  },
  nodes: {
    title: '多机舰队拓扑',
    subtitle: '编织本机、局域网与 Relay Termexo 算力节点，按标签亲和与余量调度',
    icon: 'radar',
  },
  analytics: {
    title: '效能账本与告警通知',
    subtitle: '量化难度路由节省的算力成本，追踪模型成功率与多通道告警推送',
    icon: 'chart',
  },
};

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [CommonModule, FormsModule, FleetIconComponent],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit, OnDestroy {
  protected readonly backend = inject(FleetBackendService);

  protected readonly cmdSearchInput = viewChild<ElementRef<HTMLInputElement>>('cmdSearchInput');
  protected readonly terminalOutputEl = viewChild<ElementRef<HTMLPreElement>>('terminalOutputEl');

  protected readonly activeTab = signal<FleetTab>('missions');
  protected readonly windowMaximized = signal(false);
  protected readonly sidebarCollapsed = signal(false);
  protected readonly composerCollapsed = signal(false);
  protected readonly collapsedMissions = signal<Record<string, boolean>>({});
  protected readonly slotNodeFilter = signal<string>('all');
  protected readonly confirmingRemoveNodeId = signal<string | null>(null);
  protected readonly nodeTokenVisible = signal(false);

  protected readonly toast = signal<{ text: string; tone: 'success' | 'error' | 'info' } | null>(null);
  protected readonly copiedKey = signal<string | null>(null);

  protected readonly terminalDrawer = signal<TerminalDrawerState | null>(null);
  protected readonly drawerReplyDraft = signal('');
  protected readonly drawerAutoRefresh = signal(false);

  protected readonly advancedComposerOpen = signal(false);
  protected readonly nodeModalOpen = signal(false);
  protected readonly mcpDrawerOpen = signal(false);
  protected readonly mcpRemote = signal(false);
  protected readonly mcpRemoteUrl = signal('');
  protected readonly mcpRemoteSaving = signal(false);
  protected readonly mcpRemoteError = signal('');

  protected readonly commandPaletteOpen = signal(false);
  protected readonly commandQuery = signal('');
  protected readonly commandSelectedIndex = signal(0);
  protected readonly missionFilter = signal<'all' | 'active' | 'completed' | 'failed' | 'cancelled'>('all');

  protected readonly workflowOptions = WORKFLOW_OPTIONS;
  protected readonly difficultyOptions = DIFFICULTY_OPTIONS;
  protected readonly missionPresets = MISSION_PRESETS;
  protected readonly simulatorPresets = SIMULATOR_PRESETS;
  protected readonly allTiers: DifficultyTier[] = ['L1', 'L2', 'L3', 'L4'];
  protected readonly quickReplies = [
    { label: '同意 (y)', text: 'y' },
    { label: '拒绝 (n)', text: 'n' },
    { label: '继续执行', text: '继续执行，完成后汇报改动摘要。' },
    { label: '汇报进度', text: '请先简要汇报当前已完成改动与下一步计划。' },
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
  protected readonly nodeConnectError = signal('');

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

  private toastTimer: ReturnType<typeof setTimeout> | null = null;
  private copiedTimer: ReturnType<typeof setTimeout> | null = null;
  private drawerPollTimer: ReturnType<typeof setInterval> | null = null;
  private unlistenWindowResize: UnlistenFn | null = null;

  protected readonly activeViewMeta = computed(() => VIEW_META[this.activeTab()]);

  protected readonly activeWorkflowOption = computed(
    () => this.workflowOptions.find((w) => w.id === this.selectedWorkflow()) ?? this.workflowOptions[0],
  );

  protected readonly resolvedNodeId = computed(() => {
    const nodes = this.backend.nodes();
    return nodes.find((node) => node.id === this.selectedNodeId())?.id ?? nodes[0]?.id ?? '';
  });

  protected readonly activeNodeSnapshot = computed(
    () => this.backend.snapshots()[this.resolvedNodeId()] ?? null,
  );

  protected readonly resolvedWorkspaceId = computed(() => {
    const workspaces = this.activeNodeSnapshot()?.workspaces ?? [];
    return (
      workspaces.find((workspace) => workspace.id === this.selectedWorkspaceId())?.id ??
      workspaces[0]?.id ??
      ''
    );
  });

  protected readonly selectedWorkspaceMeta = computed(() => {
    const id = this.resolvedWorkspaceId();
    return this.activeNodeSnapshot()?.workspaces.find((w) => w.id === id) ?? null;
  });

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

  protected readonly unhealthySlotCount = computed(
    () => this.backend.slots().filter((s) => s.health !== 'healthy').length,
  );

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
      const waiting = missionTasks.filter(
        (t) => t.status === 'waiting_input' || t.status === 'waiting_approval',
      ).length;
      const progress = missionTasks.length
        ? Math.round((completed / missionTasks.length) * 100)
        : 0;
      return {
        mission,
        stages: computeDagStages(missionTasks),
        tasks: missionTasks,
        completedCount: completed,
        runningCount: running,
        waitingCount: waiting,
        progress,
      };
    });
  });

  protected readonly missionStatusCounts = computed(() => {
    const list = this.missionsWithDag();
    return {
      all: list.length,
      active: list.filter((g) => g.mission.status === 'running' || g.mission.status === 'draft').length,
      completed: list.filter((g) => g.mission.status === 'completed').length,
      failed: list.filter((g) => g.mission.status === 'failed').length,
      cancelled: list.filter((g) => g.mission.status === 'cancelled').length,
    };
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

  protected readonly filteredSlots = computed(() => {
    const nodeFilter = this.slotNodeFilter();
    const slots = this.backend.slots();
    if (nodeFilter === 'all') return slots;
    return slots.filter((s) => s.nodeId === nodeFilter);
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
    return { tier, reason, winner, costUnits: this.tierCostUnits(tier) };
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
        shortcut: 'Alt 1',
        run: () => this.activeTab.set('missions'),
      },
      {
        id: 'go-tree',
        title: '子 Agent 蜂群树',
        hint: '查看父子派生树与护栏仪表盘',
        group: '导航视图',
        icon: 'tree',
        shortcut: 'Alt 2',
        run: () => this.activeTab.set('tree'),
      },
      {
        id: 'go-inbox',
        title: '全局待办收件箱',
        hint: '集中处理等待审批与等待输入的终端',
        group: '导航视图',
        icon: 'inbox',
        shortcut: 'Alt 3',
        run: () => this.activeTab.set('inbox'),
      },
      {
        id: 'go-routing',
        title: '难度天梯与账号池',
        hint: '测试 L1–L4 路由策略与调整槽位档位',
        group: '导航视图',
        icon: 'route',
        shortcut: 'Alt 4',
        run: () => this.activeTab.set('routing'),
      },
      {
        id: 'go-nodes',
        title: '多机舰队拓扑',
        hint: '管理本机、局域网/VPN 与 Relay 节点',
        group: '导航视图',
        icon: 'radar',
        shortcut: 'Alt 5',
        run: () => this.activeTab.set('nodes'),
      },
      {
        id: 'go-analytics',
        title: '效能账本与告警通知',
        hint: '查看节省比例、模型排行与 Webhook / 微信配置',
        group: '导航视图',
        icon: 'chart',
        shortcut: 'Alt 6',
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
        run: () => this.triggerRefresh(),
      },
      {
        id: 'act-mcp',
        title: '复制反向 MCP 接入 JSON',
        hint: '用于给 Termexo 配置自动子 Agent 能力',
        group: '快捷操作',
        icon: 'copy',
        run: () => void this.copyFleetMcpConfig(),
      },
      {
        id: 'act-sidebar',
        title: this.sidebarCollapsed() ? '展开左侧导航栏' : '收起左侧导航栏（宽屏模式）',
        hint: '释放更多水平空间给多阶段 DAG 流水线',
        group: '界面布局',
        icon: 'panel-left',
        shortcut: '[',
        run: () => this.toggleSidebar(),
      },
    ];

    const liveTerminalActions: CommandAction[] = this.backend
      .tasks()
      .filter((t) => Boolean(t.assignedNodeId && t.termexoTerminalId))
      .slice(0, 6)
      .map((task) => ({
        id: `term-${task.id}`,
        title: `查看终端 · ${task.title}`,
        hint: `${task.assignedNodeId} · ${task.assignedModelName || task.assignedAgentType || 'Agent'} (${this.statusLabel(task.status)})`,
        group: '实时终端',
        icon: 'terminal',
        run: () => void this.inspectTerminal(task),
      }));

    const combined = [...base, ...liveTerminalActions];
    if (!q) return combined;
    return combined.filter(
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
    return this.allTiers.map((tier) => {
      const ranked = this.rankSimulatedSlots(tier, mode);
      return {
        tier,
        meta: this.difficultyOptions.find((d) => d.id === tier)!,
        winner: ranked[0] ?? null,
        backupsCount: Math.max(0, ranked.length - 1),
        costUnits: this.tierCostUnits(tier),
      };
    });
  });

  /** Interactive simulation for any user-typed prompt in the Routing tab. */
  protected readonly simulatedRoute = computed(() => {
    const text = this.simulatorPrompt().trim().toLowerCase();
    let tier: DifficultyTier = 'L2';
    let reason = '常规研发任务，匹配标准均衡档';
    if (/(review|audit|security|审查|评审|安全|交叉验证)/.test(text)) {
      tier = 'L4';
      reason = '命中安全/审查语义，启用 L4 跨模型家族异构审查';
    } else if (
      /(architect|refactor|concurrency|deadlock|migration|performance|架构|重构|并发|死锁|性能|底层)/.test(
        text,
      ) ||
      text.length > 200
    ) {
      tier = 'L3';
      reason = '命中复杂工程/重构/并发特征，升级至 L3 顶级推理档';
    } else if (/(typo|readme|comment|format|lint|文案|注释|格式化|查看|列出|文档)/.test(text)) {
      tier = 'L1';
      reason = '轻量查询或机械改动，分配至 L1 低成本闪电档';
    }
    const winner = this.pickSimulatedSlot(tier, this.simulatorMode());
    return { tier, reason, winner, costUnits: this.tierCostUnits(tier) };
  });

  ngOnInit(): void {
    void this.backend.initialize().then(() => {
      this.syncNotificationFormFromState();
    });
    if (this.backend.desktop) {
      void this.initDesktopWindowState();
    }
  }

  ngOnDestroy(): void {
    this.stopDrawerPolling();
    if (this.toastTimer) clearTimeout(this.toastTimer);
    if (this.copiedTimer) clearTimeout(this.copiedTimer);
    this.unlistenWindowResize?.();
    this.unlistenWindowResize = null;
    this.backend.destroy();
  }

  private async initDesktopWindowState(): Promise<void> {
    try {
      const win = getCurrentWindow();
      this.windowMaximized.set(await win.isMaximized());
      this.unlistenWindowResize = await win.onResized(() => {
        void win.isMaximized().then((max) => this.windowMaximized.set(max));
      });
    } catch {
      // Non-fatal fallback if window state query is unavailable
    }
  }

  protected async minimizeWindow(): Promise<void> {
    if (!this.backend.desktop) return;
    await getCurrentWindow().minimize();
  }

  protected async toggleMaximizeWindow(): Promise<void> {
    if (!this.backend.desktop) return;
    const win = getCurrentWindow();
    await win.toggleMaximize();
    this.windowMaximized.set(await win.isMaximized());
  }

  protected async closeWindow(): Promise<void> {
    if (!this.backend.desktop) return;
    await getCurrentWindow().close();
  }

  protected toggleSidebar(): void {
    this.sidebarCollapsed.update((v) => !v);
  }

  protected toggleComposerCollapsed(): void {
    this.composerCollapsed.update((v) => !v);
  }

  protected toggleMissionCollapsed(missionId: string): void {
    this.collapsedMissions.update((map) => ({
      ...map,
      [missionId]: !map[missionId],
    }));
  }

  protected isMissionCollapsed(missionId: string): boolean {
    return Boolean(this.collapsedMissions()[missionId]);
  }

  protected triggerRefresh(): void {
    void this.backend.refresh();
    this.notify('已触发全舰队状态同步', 'info');
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
      this.notify(msg, 'error');
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

  protected formatRelativeTime(ms: number | null | undefined): string {
    if (!ms) return '尚未同步';
    const diff = Math.max(0, Date.now() - ms);
    if (diff < 15000) return '刚刚同步';
    if (diff < 60000) return `${Math.floor(diff / 1000)} 秒前`;
    if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
    return `${Math.floor(diff / 3600000)} 小时前`;
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
    this.nodeTokenVisible.set(false);
    this.nodeProbeResult.set(null);
    this.nodeConnectError.set('');
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
    this.nodeTokenVisible.set(false);
    this.nodeProbeResult.set(null);
    this.nodeConnectError.set('');
  }

  protected requestRemoveNode(nodeId: string): void {
    this.confirmingRemoveNodeId.set(nodeId);
  }

  protected cancelRemoveNode(): void {
    this.confirmingRemoveNodeId.set(null);
  }

  protected async confirmRemoveNode(nodeId: string): Promise<void> {
    this.confirmingRemoveNodeId.set(null);
    await this.backend.removeNode(nodeId);
    this.notify('已从舰队拓扑中移除该节点', 'info');
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
    if (this.backend.busy()) return;
    this.nodeConnectError.set('');
    const tags = this.nodeTags()
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    try {
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
    } catch (error) {
      this.nodeConnectError.set(error instanceof Error ? error.message : String(error));
      return;
    }
    this.nodeModalOpen.set(false);
    this.resetNodeForm();
    this.notify('Termexo 节点已连接');
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

  protected transportLabel(t: TransportType): string {
    switch (t) {
      case 'local':
        return '本机直连';
      case 'lan':
        return '局域网 / VPN';
      case 'relay':
        return 'Relay 隧道';
      default:
        return t;
    }
  }

  protected routingModeLabel(mode: RoutingMode): string {
    switch (mode) {
      case 'cost':
        return '省钱优先';
      case 'quality':
        return '质量优先';
      default:
        return '均衡模式';
    }
  }

  protected tierCostUnits(tier: DifficultyTier): string {
    switch (tier) {
      case 'L1':
        return '1 u';
      case 'L2':
        return '3 u';
      case 'L3':
        return '8 u';
      case 'L4':
        return '8 u';
    }
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

  protected attentionKindLabel(kind: AttentionKind): string {
    const map: Record<AttentionKind, string> = {
      waiting_approval: '等待人工审批',
      waiting_input: '等待终端输入',
      rate_limited: '触发限流冷却',
      auth_expired: '账号登录过期',
      task_failed: '任务执行失败',
    };
    return map[kind] ?? kind;
  }

  protected attentionKindTone(kind: AttentionKind): string {
    switch (kind) {
      case 'waiting_approval':
      case 'waiting_input':
        return 'warn';
      case 'rate_limited':
        return 'info';
      case 'auth_expired':
      case 'task_failed':
        return 'danger';
      default:
        return 'warn';
    }
  }

  protected slotsRange(count: number): number[] {
    return Array.from({ length: Math.max(0, count) }, (_, i) => i);
  }

  protected percent(used: number, max: number): number {
    return Math.min(100, Math.round((used / Math.max(1, max)) * 100));
  }

  protected isCopied(key: string): boolean {
    return this.copiedKey() === key;
  }

  protected async copyText(key: string, text: string, successMsg: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.copiedKey.set(key);
      if (this.copiedTimer) clearTimeout(this.copiedTimer);
      this.copiedTimer = setTimeout(() => {
        if (this.copiedKey() === key) this.copiedKey.set(null);
      }, 1700);
      this.notify(successMsg);
    } catch {
      this.notify('剪贴板不可用，请手动复制', 'error');
    }
  }

  protected openCommandPalette(): void {
    this.commandQuery.set('');
    this.commandSelectedIndex.set(0);
    this.commandPaletteOpen.set(true);
    setTimeout(() => {
      this.cmdSearchInput()?.nativeElement.focus();
    }, 10);
  }

  protected updateCommandQuery(q: string): void {
    this.commandQuery.set(q);
    this.commandSelectedIndex.set(0);
  }

  protected handlePaletteKeydown(event: KeyboardEvent): void {
    const items = this.filteredCommands();
    if (!items.length) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      this.commandSelectedIndex.update((i) => (i + 1) % items.length);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      this.commandSelectedIndex.update((i) => (i - 1 + items.length) % items.length);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const target = items[this.commandSelectedIndex()] ?? items[0];
      if (target) this.runCommandAction(target);
    }
  }

  @HostListener('window:keydown', ['$event'])
  protected handleGlobalKeydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      if (this.commandPaletteOpen()) {
        this.commandPaletteOpen.set(false);
      } else {
        this.openCommandPalette();
      }
      return;
    }

    if (event.key === 'Escape') {
      if (this.commandPaletteOpen()) this.commandPaletteOpen.set(false);
      else if (this.terminalDrawer()) this.closeTerminalDrawer();
      else if (this.nodeModalOpen()) this.nodeModalOpen.set(false);
      else if (this.mcpDrawerOpen()) this.mcpDrawerOpen.set(false);
      else if (this.confirmingRemoveNodeId()) this.confirmingRemoveNodeId.set(null);
      return;
    }

    // Alt + 1..6 quick view switching
    if (event.altKey && !event.ctrlKey && !event.metaKey) {
      const tabOrder: FleetTab[] = ['missions', 'tree', 'inbox', 'routing', 'nodes', 'analytics'];
      const idx = Number(event.key) - 1;
      if (idx >= 0 && idx < tabOrder.length) {
        event.preventDefault();
        const tab = tabOrder[idx];
        if (tab === 'analytics') this.openAnalyticsTab();
        else this.activeTab.set(tab);
        return;
      }
    }

    // `[` toggles sidebar when not typing in a form field
    const tag = (event.target as HTMLElement | null)?.tagName?.toLowerCase();
    const isTyping = tag === 'input' || tag === 'textarea' || tag === 'select';
    if (!isTyping && !event.ctrlKey && !event.metaKey && !event.altKey && event.key === '[') {
      event.preventDefault();
      this.toggleSidebar();
    }
  }

  protected runCommandAction(action: CommandAction): void {
    this.commandPaletteOpen.set(false);
    this.commandQuery.set('');
    this.commandSelectedIndex.set(0);
    action.run();
  }

  protected applyMissionPreset(preset: MissionPreset): void {
    this.composerCollapsed.set(false);
    this.missionTitle.set(preset.title);
    this.missionGoal.set(preset.goal);
    this.selectWorkflow(preset.workflow);
    this.selectedDifficulty.set(preset.difficulty);
  }

  protected clearComposer(): void {
    this.missionTitle.set('');
    this.missionGoal.set('');
  }

  protected selectWorkflow(workflow: WorkflowTemplate): void {
    this.selectedWorkflow.set(workflow);
    if (workflow === 'planner_dag') {
      this.selectedExecutionMode.set('task');
    }
  }

  protected selectMissionNode(nodeId: string): void {
    this.selectedNodeId.set(nodeId);
    this.selectedWorkspaceId.set('');
    this.selectedMcpProfileId.set('');
  }

  protected dependenciesCompleted(task: MissionTask, tasks: MissionTask[]): boolean {
    return task.dependsOn.every((id) => tasks.some((item) => item.id === id && item.status === 'completed'));
  }

  protected handleGoalKeydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
      event.preventDefault();
      void this.launchMission();
    }
  }

  protected applyNodePreset(kind: 'local' | 'lan' | 'tailscale'): void {
    this.nodeProbeResult.set(null);
    this.nodeConnectError.set('');
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
    if (this.backend.busy()) return;
    const goal = this.missionGoal().trim();
    const rawTitle = this.missionTitle().trim();
    const title = rawTitle || goal.split(/\r?\n/)[0].slice(0, 28);
    if (!title || !goal) {
      this.notify('请输入任务目标描述（按 Ctrl+Enter 可直接下发）', 'info');
      return;
    }
    const nodeId = this.resolvedNodeId();
    const workspaceId = this.resolvedWorkspaceId();
    if (!nodeId || !workspaceId) {
      this.openNewNodeModal();
      this.notify('请先连接至少一台本机或远程 Termexo', 'error');
      return;
    }
    const mcpProfile =
      this.availableMcpProfiles().find((p) => p.id === this.selectedMcpProfileId())?.id ||
      this.availableMcpProfiles().find((p) => p.name.toLowerCase() === 'termexo fleet')?.id ||
      null;
    const requiredNodeTags = this.requiredTagsInput()
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    try {
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
          maxDepth: this.maxDepth() ?? 2,
          maxConcurrency: this.maxConcurrency() ?? 4,
          maxSubagents: this.maxSubagents() ?? 12,
          isolateWorktreeByDefault: this.isolateWorktree(),
        },
        fleetMcpProfileId: mcpProfile,
        autoDispatch: true,
      });
    } catch (error) {
      this.notify(error instanceof Error ? error.message : String(error), 'error');
      return;
    }
    this.missionTitle.set('');
    this.missionGoal.set('');
    this.notify('任务已按难度路由并下发至 Termexo 执行');
  }

  protected openMcpDrawer(): void {
    this.mcpRemote.set(false);
    this.mcpRemoteUrl.set(this.backend.overview().mcpRemoteUrl ?? '');
    this.mcpRemoteError.set('');
    this.mcpDrawerOpen.set(true);
  }

  protected async saveMcpRemoteUrl(): Promise<void> {
    this.mcpRemoteSaving.set(true);
    this.mcpRemoteError.set('');
    try {
      const url = await this.backend.saveMcpRemoteUrl(this.mcpRemoteUrl());
      this.mcpRemoteUrl.set(url ?? '');
      this.notify('远程反向 MCP 地址已保存');
    } catch (error) {
      this.mcpRemoteError.set(error instanceof Error ? error.message : String(error));
    } finally {
      this.mcpRemoteSaving.set(false);
    }
  }

  protected async copyFleetMcpConfig(remote = false): Promise<void> {
    const snippet = this.backend.buildTermexoMcpProfileSnippet(remote);
    await this.copyText(
      remote ? 'mcp-remote' : 'mcp-local',
      snippet,
      '已复制反向 MCP JSON，粘贴到 Termexo「设置 → MCP Profile」即可',
    );
  }

  protected async inspectTerminal(task: MissionTask): Promise<void> {
    if (!task.assignedNodeId || !task.termexoTerminalId) return;
    await this.openTerminalStream({
      nodeId: task.assignedNodeId,
      terminalId: task.termexoTerminalId,
      title: task.title,
      model: task.assignedModelName || this.agentBadge(task.assignedAgentType).name,
      status: task.status,
      branch: task.worktreeBranch,
    });
  }

  protected async inspectAttentionTerminal(item: AttentionItem): Promise<void> {
    if (!item.terminalId) return;
    await this.openTerminalStream({
      nodeId: item.nodeId,
      terminalId: item.terminalId,
      title: item.title,
      model: item.nodeName,
      status: item.kind,
      branch: null,
    });
  }

  private async openTerminalStream(meta: {
    nodeId: string;
    terminalId: string;
    title: string;
    model: string;
    status: string;
    branch: string | null;
  }): Promise<void> {
    this.stopDrawerPolling();
    this.drawerAutoRefresh.set(false);
    this.drawerReplyDraft.set('');
    this.terminalDrawer.set({
      ...meta,
      text: '正在拉取 Termexo 实时终端输出...',
      loading: true,
      updatedAtMs: Date.now(),
    });
    try {
      const text = await this.backend.readTerminalTail(meta.nodeId, meta.terminalId);
      this.terminalDrawer.update((cur) =>
        cur ? { ...cur, text, loading: false, updatedAtMs: Date.now() } : null,
      );
      this.scrollTerminalToBottom();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.terminalDrawer.update((cur) =>
        cur ? { ...cur, text: `读取失败：${msg}`, loading: false, updatedAtMs: Date.now() } : null,
      );
    }
  }

  protected closeTerminalDrawer(): void {
    this.stopDrawerPolling();
    this.drawerAutoRefresh.set(false);
    this.terminalDrawer.set(null);
  }

  protected toggleDrawerAutoRefresh(): void {
    const next = !this.drawerAutoRefresh();
    this.drawerAutoRefresh.set(next);
    if (next) {
      this.stopDrawerPolling();
      this.drawerPollTimer = setInterval(() => {
        if (this.terminalDrawer()) {
          void this.refreshTerminalDrawer(true);
        } else {
          this.stopDrawerPolling();
        }
      }, 3000);
      this.notify('已开启终端每 3 秒自动刷新', 'info');
    } else {
      this.stopDrawerPolling();
    }
  }

  private stopDrawerPolling(): void {
    if (this.drawerPollTimer) {
      clearInterval(this.drawerPollTimer);
      this.drawerPollTimer = null;
    }
  }

  private scrollTerminalToBottom(): void {
    setTimeout(() => {
      const el = this.terminalOutputEl()?.nativeElement;
      if (el) {
        el.scrollTop = el.scrollHeight;
      }
    }, 20);
  }

  protected async refreshTerminalDrawer(silent = false): Promise<void> {
    const cur = this.terminalDrawer();
    if (!cur) return;
    if (!silent) {
      this.terminalDrawer.set({ ...cur, loading: true });
    }
    try {
      const text = await this.backend.readTerminalTail(cur.nodeId, cur.terminalId);
      this.terminalDrawer.update((state) =>
        state ? { ...state, text, loading: false, updatedAtMs: Date.now() } : null,
      );
      this.scrollTerminalToBottom();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.terminalDrawer.update((state) =>
        state ? { ...state, text: `读取失败：${msg}`, loading: false, updatedAtMs: Date.now() } : null,
      );
    }
  }

  protected async copyTerminalOutput(): Promise<void> {
    const cur = this.terminalDrawer();
    if (!cur) return;
    await this.copyText('terminal-out', cur.text, '已复制当前终端输出');
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

  private rankSimulatedSlots(tier: DifficultyTier, mode: RoutingMode): AccountModelSlot[] {
    const healthy = this.backend
      .slots()
      .filter((s) => s.health === 'healthy' && s.activeCount < Math.max(1, s.maxConcurrency));
    if (!healthy.length) return [];
    const exact = healthy.filter((s) => !s.supportedTiers.length || s.supportedTiers.includes(tier));
    const pool = exact.length ? exact : healthy;
    const score = (slot: AccountModelSlot): number => {
      const modeScore =
        mode === 'cost'
          ? (6 - Math.min(5, slot.costTier)) * 120 + slot.qualityTier * 20
          : mode === 'quality'
            ? slot.qualityTier * 140 - slot.costTier * 10
            : 150 -
              Math.abs(slot.qualityTier - (tier === 'L1' ? 2 : tier === 'L2' ? 3 : 5)) * 35 +
              (6 - slot.costTier) * 30;
      return modeScore + slot.priority * 10 - slot.activeCount * 60;
    };
    return [...pool].sort((a, b) => score(b) - score(a));
  }

  private pickSimulatedSlot(tier: DifficultyTier, mode: RoutingMode): AccountModelSlot | null {
    return this.rankSimulatedSlots(tier, mode)[0] ?? null;
  }

  private notify(text: string, tone: 'success' | 'error' | 'info' = 'success'): void {
    this.toast.set({ text, tone });
    if (this.toastTimer) clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => {
      if (this.toast()?.text === text) {
        this.toast.set(null);
      }
    }, 3600);
  }
}
