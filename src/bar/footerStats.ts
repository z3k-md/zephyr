// The footer's meters. Readings arrive twice a second while the bar shows (stats.rs); each is
// eased toward, so the bars glide instead of jumping, and a shown percentage only changes
// when the eased value has moved most of a point, so the digits don't flicker.

import { computed, shallowRef } from 'vue';
import { bytes } from '../format';
import type { SystemStats } from '../types';

/** Seconds for the eased value to cover most of a change. */
const EASE = { cpu: 1.0, memory: 1.5, network: 1.5 };
/** How far the eased value moves before the shown percentage follows. */
const STEP = 0.75;
/** A longer gap (the bar was hidden, or another view showed) starts afresh. */
const GAP_MS = 3000;
/** Network activity on a log scale: 2 KB/s is idle, 50 MB/s is full. */
const IDLE = 2048;
const FULL = 50 * 1024 * 1024;

interface Eased {
  at: number;
  cpu: number;
  /** Memory in use, 0–100. */
  memory: number;
  down: number;
  up: number;
  shownCpu: number;
  shownMemory: number;
}

function ease(from: number, to: number, seconds: number, tau: number): number {
  return from + (1 - Math.exp(-seconds / tau)) * (to - from);
}

function level(rate: number): number {
  const value = Math.log10(Math.max(rate, 1) / IDLE) / Math.log10(FULL / IDLE);
  return Math.min(1, Math.max(0, value));
}

function percentUsed(stats: SystemStats): number {
  return stats.memoryTotal > 0 ? (100 * stats.memoryUsed) / stats.memoryTotal : 0;
}

export function useFooterStats() {
  /** The latest reading, for exact numbers in titles and the dev build's own use. */
  const latest = shallowRef<SystemStats | null>(null);
  const eased = shallowRef<Eased | null>(null);

  function push(next: SystemStats) {
    const now = performance.now();
    const before = eased.value;
    latest.value = next;
    if (!before || now - before.at > GAP_MS) {
      const cpu = next.cpu;
      const memory = percentUsed(next);
      eased.value = {
        at: now,
        cpu,
        memory,
        down: next.netDown,
        up: next.netUp,
        shownCpu: Math.round(cpu),
        shownMemory: Math.round(memory),
      };
      return;
    }
    const seconds = (now - before.at) / 1000;
    const cpu = ease(before.cpu, next.cpu, seconds, EASE.cpu);
    const memory = ease(before.memory, percentUsed(next), seconds, EASE.memory);
    eased.value = {
      at: now,
      cpu,
      memory,
      down: ease(before.down, next.netDown, seconds, EASE.network),
      up: ease(before.up, next.netUp, seconds, EASE.network),
      shownCpu: Math.abs(cpu - before.shownCpu) >= STEP ? Math.round(cpu) : before.shownCpu,
      shownMemory:
        Math.abs(memory - before.shownMemory) >= STEP ? Math.round(memory) : before.shownMemory,
    };
  }

  const footer = computed(() => {
    const stats = latest.value;
    const now = eased.value;
    if (!stats || !now) return null;
    const diskShare = stats.diskTotal > 0 ? stats.diskFree / stats.diskTotal : 1;
    return {
      cpu: `${now.shownCpu}%`,
      cpuScale: Math.max(0.02, Math.min(1, now.cpu / 100)),
      cpuHot: now.shownCpu >= 85,
      memory: `${now.shownMemory}%`,
      memoryScale: Math.max(0.02, Math.min(1, now.memory / 100)),
      memoryTitle: `Memory: ${bytes(stats.memoryUsed)} of ${bytes(stats.memoryTotal, 0)}`,
      diskLow: diskShare < 0.1,
      disk: `${Math.round(diskShare * 100)}% free`,
      diskTitle: `${bytes(stats.diskFree)} free of ${bytes(stats.diskTotal, 0)} on the system disk`,
      down: level(now.down),
      up: level(now.up),
      networkTitle: `Network: ↓ ${bytes(stats.netDown)}/s · ↑ ${bytes(stats.netUp)}/s`,
      app: stats.app,
    };
  });

  return { push, footer };
}
