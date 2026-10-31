import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  flagged,
  issues,
  logDirs,
  parseLog,
  report,
  signature,
  sinceStamp,
  stamp,
} from './logscan.mjs';

const LOG = `2026-10-30 10:00:00.000 INFO [zephyr_lib::logger] Zephyr 2.10.0 (release) starting on windows x86_64, pid 10
2026-10-30 10:00:01.000 ERROR [zephyr_lib::clipboard] couldn't read clipboard history, starting over: bad key 12
2026-10-30 10:00:02.000 ERROR [zephyr_lib::clipboard] couldn't read clipboard history, starting over: bad key 13
2026-10-30 10:00:03.000 ERROR [zephyr_lib::logger] panic on thread main: panicked at src/app.rs:1358:8:
called \`Option::unwrap()\` on a \`None\` value
   0: std::backtrace::Backtrace::force_capture
2026-10-30 10:00:04.000 INFO [zephyr_lib::logger] Zephyr 2.10.0 (release) starting on windows x86_64, pid 11
2026-10-30 10:00:05.000 WARN [webview] bar /: command app_icon(appId, size) failed: no icon
2026-10-30 10:00:06.000 INFO [zephyr_lib] Zephyr exiting
2026-10-30 10:00:07.000 INFO [zephyr_lib::logger] Zephyr 2.10.0 (release) starting on windows x86_64, pid 12
`;

test('a line without a timestamp joins the entry before it', () => {
  const entries = parseLog(LOG, 'release');
  assert.equal(entries.length, 8);
  assert.match(entries[3].message, /None` value\n {3}0: std::backtrace/);
});

test('numbers do not split an issue, and a panic keeps what it was', () => {
  const [first, second, panic] = parseLog(LOG, 'release').slice(1, 4);
  assert.equal(signature(first), signature(second));
  assert.match(
    signature(panic),
    /^ERROR \[zephyr_lib::logger\] panic: panicked at src\/app\.rs:#:#: called/
  );
});

test('issues groups, ranks panics first and finds the run that never exited', () => {
  const groups = issues(flagged(parseLog(LOG, 'release')));
  assert.deepEqual(
    groups.map((group) => [group.sample.message.split('\n')[0].slice(0, 30), group.count]),
    [
      ['panic on thread main: panicked', 1],
      ["couldn't read clipboard histor", 2],
      ['bar /: command app_icon(appId,', 1],
      ['the run started 2026-10-30 10:', 1],
    ]
  );
  // pid 11 exited cleanly; pid 12 is still running, so neither counts. Pid 10's is dated when
  // pid 11 started.
  assert.match(groups[3].sample.message, /started 2026-10-30 10:00:00\.000/);
  assert.match(groups[3].sample.time, /10:00:04/);
  assert.match(report(groups), /2× 2026-10-30 10:00:01\.000 → 2026-10-30 10:00:02\.000/);
});

test('a scan made while a run was up still reports it crashing later', () => {
  const entries = parseLog(LOG, 'release');
  const later = flagged(entries, '2026-10-30 10:00:03.500');
  assert.deepEqual(
    later.map((entry) => entry.message.slice(0, 25)),
    ['bar /: command app_icon(a', 'the run started 2026-10-3']
  );
  // Dev builds are killed at every rebuild, so theirs never count.
  assert.equal(flagged(parseLog(LOG, 'dev')).length, 4);
});

test('sinceStamp and stamp write the log’s own time format', () => {
  const now = new Date(2026, 9, 30, 12, 0, 0, 5);
  assert.equal(stamp(now), '2026-10-30 12:00:00.005');
  assert.equal(sinceStamp('2h', now), '2026-10-30 10:00:00.005');
  assert.throws(() => sinceStamp('soon', now));
});

test('dev builds log beside the installed app', () => {
  const dirs = logDirs({ LOCALAPPDATA: 'C:\\Users\\me\\AppData\\Local' }, 'win32');
  assert.match(dirs.release, /Zephyr[\\/]logs$/);
  assert.match(dirs.dev, /Zephyr[\\/]logs[\\/]dev$/);
});
