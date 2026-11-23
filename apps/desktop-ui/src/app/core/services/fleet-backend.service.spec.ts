import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';

import { computeDagStages, FleetBackendService } from './fleet-backend.service';

describe('FleetBackendService', () => {
  it('supports multi-node upsert, 3-stage DAG creation, escalation retry and topological layering', async () => {
    TestBed.configureTestingModule({});
    const service = TestBed.inject(FleetBackendService);

    expect(service.desktop).toBe(false);
    expect(service.nodes().length).toBeGreaterThanOrEqual(2);

    const added = await service.upsertNode({
      name: 'Linux CI',
      transportType: 'relay',
      endpoint: 'https://relay.example.com/node/linux/mcp',
      tags: ['linux', 'ci'],
      maxConcurrency: 6,
    });
    expect(service.nodes().some((n) => n.id === added.id)).toBe(true);

    const [mission, firstTask] = await service.createMission({
      title: 'Add OAuth2 support',
      goalPrompt: 'Implement OAuth2 login, run tests and perform cross-model review',
      nodeId: 'node-local',
      workspaceId: 'ws-termexo',
      projectId: 'proj-default',
      routingMode: 'balanced',
      workflowTemplate: 'implement_verify_review',
      requiredNodeTags: ['windows'],
      executionMode: 'task',
      preferredTerminalId: null,
      difficulty: 'L2',
      preferredAgent: null,
      guardrails: {
        maxDepth: 2,
        maxConcurrency: 4,
        maxSubagents: 12,
        isolateWorktreeByDefault: true,
      },
      fleetMcpProfileId: 'mcp-fleet',
      autoDispatch: true,
    });
    expect(mission.status).toBe('running');
    expect(firstTask.difficulty).toBe('L2');

    const missionTasks = service.tasks().filter((t) => t.missionId === mission.id);
    expect(missionTasks).toHaveLength(3);
    const stages = computeDagStages(missionTasks);
    expect(stages.map((s) => s.stage)).toEqual([0, 1, 2]);
    expect(stages[2].tasks[0].difficulty).toBe('L4');

    // Retry with automatic difficulty escalation (L2 -> L3).
    await service.retryTaskWithEscalation(firstTask.id);
    const escalated = service.tasks().find((t) => t.id === firstTask.id)!;
    expect(escalated.difficulty).toBe('L3');
    expect(escalated.retryCount).toBe(1);

    const snippet = JSON.parse(service.buildTermexoMcpProfileSnippet());
    expect(snippet.mcpServers['termexo-fleet'].url).toContain('/mcp');

    // Phase 3: connectivity probe, analytics ledger & notification settings
    const probe = await service.probeNode('https://192.168.1.50:7421/mcp', 'tok', true);
    expect(probe.reachable).toBe(true);
    expect(probe.supportsMcpProfiles).toBe(true);

    expect(service.analytics().totalRuns).toBeGreaterThanOrEqual(3);
    expect(service.analytics().estimatedSavingsPercent).toBeGreaterThan(0);

    const savedSettings = await service.saveNotificationSettings({
      ...service.notificationSettings(),
      webhookEnabled: true,
      webhookUrl: 'https://hooks.example.com/fleet',
    });
    expect(savedSettings.webhookEnabled).toBe(true);
    const testMsg = await service.sendTestNotification();
    expect(testMsg).toContain('测试通知');
  });
});
