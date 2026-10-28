// The footer's meters. Readings arrive twice a second while the bar shows (stats.rs); each is
// eased toward, so the bars glide instead of jumping, and a shown percentage only changes
// when the eased value has moved most of a point, so the digits don't flicker.

import { computed, shallowRef } from 'vue';
import { bytes } from '../format';
import type { SystemStats } from '../types';

/** Seconds for the eased value to cover most of a change. */
const EASE = { cpu: 1.0, gpu: 1.0, memory: 1.5 };
/** How far the eased value moves before the shown percentage follows. */
const STEP = 0.75;
/** A longer gap (the bar was hidden, or another view showed) starts afresh. */
const GAP_MS = 3000;

interface Eased {
  at: number;
  cpu: number;
  /** 0–100; 0 without a GPU. */
  gpu: number;
  /** Memory in use, 0–100. */
  memory: number;
  shownCpu: number;
  shownGpu: number;
  shownMemory: number;
}

function ease(from: number, to: number, seconds: number, tau: number): number {
  return from + (1 - Math.exp(-seconds / tau)) * (to - from);
}

function percentUsed(stats: SystemStats): number {
  return stats.memoryTotal > 0 ? (100 * stats.memoryUsed) / stats.memoryTotal : 0;
}

/** A shown percentage that only follows the eased value once it has moved most of a point. */
function shown(value: number, before: number): number {
  return Math.abs(value - before) >= STEP ? Math.round(value) : before;
}

export function useFooterStats() {
  /** The latest reading, for exact numbers in titles. */
  const latest = shallowRef<SystemStats | null>(null);
  const eased = shallowRef<Eased | null>(null);

  function push(next: SystemStats) {
    const now = performance.now();
    const before = eased.value;
    latest.value = next;
    const gpu = next.gpu?.usage ?? 0;
    if (!before || now - before.at > GAP_MS) {
      const cpu = next.cpu;
      const memory = percentUsed(next);
      eased.value = {
        at: now,
        cpu,
        gpu,
        memory,
        shownCpu: Math.round(cpu),
        shownGpu: Math.round(gpu),
        shownMemory: Math.round(memory),
      };
      return;
    }
    const seconds = (now - before.at) / 1000;
    const cpu = ease(before.cpu, next.cpu, seconds, EASE.cpu);
    const easedGpu = ease(before.gpu, gpu, seconds, EASE.gpu);
    const memory = ease(before.memory, percentUsed(next), seconds, EASE.memory);
    eased.value = {
      at: now,
      cpu,
      gpu: easedGpu,
      memory,
      shownCpu: shown(cpu, before.shownCpu),
      shownGpu: shown(easedGpu, before.shownGpu),
      shownMemory: shown(memory, before.shownMemory),
    };
  }

  const footer = computed(() => {
    const stats = latest.value;
    const now = eased.value;
    if (!stats || !now) return null;
    const diskShare = stats.diskTotal > 0 ? stats.diskFree / stats.diskTotal : 1;
    const gpu = stats.gpu;
    return {
      cpu: `${now.shownCpu}%`,
      cpuScale: Math.max(0.02, Math.min(1, now.cpu / 100)),
      cpuHot: now.shownCpu >= 85,
      gpu: gpu ? `${now.shownGpu}%` : null,
      gpuScale: Math.max(0.02, Math.min(1, now.gpu / 100)),
      gpuHot: now.shownGpu >= 85,
      gpuTitle: gpu ? gpuTitle(gpu.name, now.shownGpu, gpu.memoryUsed, gpu.memoryTotal) : '',
      memory: `${now.shownMemory}%`,
      memoryScale: Math.max(0.02, Math.min(1, now.memory / 100)),
      memoryTitle: `Memory ${now.shownMemory}%\n${bytes(stats.memoryUsed)} of ${bytes(stats.memoryTotal, 0)}`,
      diskLow: diskShare < 0.1,
      disk: `${Math.round(diskShare * 100)}% free`,
      diskTitle: `${bytes(stats.diskFree)} free of ${bytes(stats.diskTotal, 0)} on the system disk`,
      network: stats.network ?? null,
    };
  });

  return { push, footer };
}

/** "GPU 12%" over "NVIDIA GeForce RTX 4070 · 1.8 GB of 12 GB memory", for a tooltip. */
export function gpuTitle(name: string, usage: number, used: number, total: number): string {
  const memory =
    total > 0
      ? `${bytes(used)} of ${bytes(total, 0)} memory`
      : used > 0
        ? `${bytes(used)} memory`
        : '';
  const detail = [name, memory].filter(Boolean).join(' · ');
  return `GPU ${Math.round(usage)}%${detail ? `\n${detail}` : ''}`;
}
