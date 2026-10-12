/** Measure unchanged native executables. Compilation is deliberately excluded. */
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const options = {};
for (let index = 2; index < process.argv.length; index += 2) {
  const key = process.argv[index];
  const value = process.argv[index + 1];
  if (!key?.startsWith('--') || value === undefined) throw new Error('Expected --option value pairs');
  options[key.slice(2)] = value;
}
if (!options.exe || !options.output || !options.profile || !options.cache) {
  throw new Error('Usage: node scripts/measure-startup.mjs --exe PATH --output DIRECTORY --profile dev|release --cache warm|cold|first [--runs 5] [--webview-profile reused|fresh-per-run] [--verification before-each|after-batch --expected-exe-sha SHA --expected-document-sha SHA] [--locale en] [--document PATH --document-mode startup|runtime] [--runtime-dir DIRECTORY] [--label baseline] [--timeout 120000]');
}
const runs = Number(options.runs ?? 5);
const timeout = Number(options.timeout ?? 120000);
const warmups = Number(options.warmups ?? (options.cache === 'warm' ? 1 : 0));
if (!Number.isInteger(runs) || runs < 1 || runs > 50 || !Number.isInteger(warmups) || warmups < 0 || warmups > 10 || !Number.isFinite(timeout) || timeout < 1000) {
  throw new Error('Invalid runs or timeout');
}
if (!['en', 'zh-CN', 'es', 'de'].includes(options.locale ?? 'en')) throw new Error('Unsupported locale');
if (!['warm', 'cold', 'first'].includes(options.cache)) throw new Error('Expected cache condition warm, cold, or first');
const webviewProfilePolicy = options['webview-profile'] ?? 'reused';
if (!['reused', 'fresh-per-run'].includes(webviewProfilePolicy)) throw new Error('Expected WebView profile policy reused or fresh-per-run');
const verificationPolicy = options.verification ?? 'before-each';
if (!['before-each', 'after-batch'].includes(verificationPolicy)) throw new Error('Expected verification policy before-each or after-batch');
if (verificationPolicy === 'after-batch' && (!/^[a-f\d]{64}$/i.test(options['expected-exe-sha'] ?? '')
  || (options.document && !/^[a-f\d]{64}$/i.test(options['expected-document-sha'] ?? '')))) {
  throw new Error('After-batch verification requires trusted expected-exe-sha and expected-document-sha when a document is supplied');
}
const documentMode = options['document-mode'] ?? 'startup';
if (!['startup', 'runtime'].includes(documentMode) || (documentMode === 'runtime' && !options.document)) {
  throw new Error('Expected document-mode startup or runtime; runtime requires --document');
}
const executable = path.resolve(options.exe);
const directory = path.resolve(options.output);
await fs.mkdir(directory, { recursive: true });
const userDataDirectory = path.join(directory, 'webview2-user-data');
await fs.mkdir(userDataDirectory);
const sha256 = async (file) => createHash('sha256').update(await fs.readFile(file)).digest('hex');
const executableHash = verificationPolicy === 'after-batch' ? options['expected-exe-sha'].toLowerCase() : await sha256(executable);
const documentPath = options.document ? path.resolve(options.document) : null;
const metadata = {
  schema: 1,
  label: options.label ?? 'startup',
  executable,
  executableSha256: executableHash,
  profile: options.profile,
  cacheCondition: options.cache,
  // Cache labels describe preparation by the operator. This script neither
  // clears OS/driver caches nor touches any shared application installation.
  cachePreparation: {
    webviewProfilePolicy,
    webviewProfileInitiallyEmpty: true,
    osAndDriverCaches: 'untouched; temperature unverified',
    inputFileCache: verificationPolicy === 'before-each'
      ? 'Executable and document hashes read entire inputs before each launch; filesystem pages may be warm'
      : 'No input reads by harness before launches; prior trusted hashes checked after batch; OS/file/driver caches untouched and unverified',
    compilationIncluded: false,
  },
  locale: options.locale ?? 'en',
  document: documentPath,
  documentMode,
  documentSha256: documentPath ? (verificationPolicy === 'after-batch' ? options['expected-document-sha'].toLowerCase() : await sha256(documentPath)) : null,
  inputVerification: { policy: verificationPolicy, status: 'pending-final-check' },
  measurementClocks: {
    launchEndpoints: 'Native process clock plus launch-to-main offset; frontend events use native IPC receipt time',
    frontendSpans: 'Exact WebView clock bounds are retained in events when available; durations do not require a cross-clock conversion',
    fileReadyMs: 'Launch-relative IPC receipt of successful file frame, not the WebView operation endpoint',
  },
  hardware: { hostname: os.hostname(), platform: process.platform, architecture: process.arch, cpu: os.cpus()[0]?.model, logicalCpus: os.cpus().length, memoryBytes: os.totalmem() },
  hostUptimeSeconds: os.uptime(),
  startedAt: new Date().toISOString(),
  userDataDirectory,
  warmups: [],
  runs: [],
};

function parseReport(contents) {
  // Accept a snapshot {events:[...]} and JSONL events so baseline and candidate
  // can share one instrumentation contract without relying on log formatting.
  try {
    const value = JSON.parse(contents);
    return Array.isArray(value) ? value : value.events ?? [value];
  } catch {
    return contents.split(/\r?\n/).filter(Boolean).flatMap((line) => {
      try { return [JSON.parse(line)]; } catch { return []; }
    });
  }
}

function phase(events, name) {
  return events.find((event) => event.phase === name && Number.isFinite(event.processElapsedMs));
}

for (let attempt = 1; attempt <= warmups + runs; attempt += 1) {
  const isWarmup = attempt <= warmups;
  const index = isWarmup ? attempt : attempt - warmups;
  if (verificationPolicy === 'before-each') {
    if (await sha256(executable) !== executableHash) throw new Error('Executable changed between runs');
    if (documentPath && await sha256(documentPath) !== metadata.documentSha256) throw new Error('Document changed between runs');
  }
  const runDirectory = path.join(directory, `${isWarmup ? 'warmup' : 'run'}-${String(index).padStart(2, '0')}`);
  // Never reuse reports from a previous invocation.
  await fs.mkdir(runDirectory);
  const runUserDataDirectory = webviewProfilePolicy === 'fresh-per-run'
    ? path.join(userDataDirectory, path.basename(runDirectory)) : userDataDirectory;
  if (webviewProfilePolicy === 'fresh-per-run') await fs.mkdir(runUserDataDirectory);
  const reportFile = path.join(runDirectory, 'startup.jsonl');
  const stdout = await fs.open(path.join(runDirectory, 'stdout.log'), 'wx');
  const stderr = await fs.open(path.join(runDirectory, 'stderr.log'), 'wx');
  const launchUnixMs = Date.now();
  const started = performance.now();
  const child = spawn(executable, [], {
    cwd: path.dirname(executable),
    windowsHide: true,
    stdio: ['ignore', stdout.fd, stderr.fd],
    env: {
      ...process.env,
      NBCAD_STARTUP_REPORT_FILE: reportFile,
      NBCAD_STARTUP_LOCALE: metadata.locale,
      NBCAD_STARTUP_DOCUMENT_MODE: documentMode,
      // WebView2's documented environment override prevents fixture launches
      // from touching the user's preferences, recovery files, or app storage.
      WEBVIEW2_USER_DATA_FOLDER: runUserDataDirectory,
      ...(documentPath ? { NBCAD_STARTUP_DOCUMENT: documentPath } : {}),
      ...(options['runtime-dir'] ? { PATH: `${path.resolve(options['runtime-dir'])}${path.delimiter}${process.env.PATH ?? ''}` } : {}),
    },
  });
  let spawnError;
  child.on('error', (error) => { spawnError = error; });
  const closed = new Promise((resolve) => child.on('close', resolve));
  const result = { index, pid: child.pid, launchUnixMs, userDataDirectory: runUserDataDirectory, events: [] };
  try {
    while (performance.now() - started < timeout) {
      if (spawnError) throw spawnError;
      try { result.events = parseReport(await fs.readFile(reportFile, 'utf8')); }
      catch (error) { if (error.code !== 'ENOENT') throw error; }
      const failure = result.events.find((event) => event.phase === 'error');
      if (failure) {
        result.status = 'error';
        result.error = failure.detail ?? 'Diagnostic interaction failed';
        break;
      }
      if (phase(result.events, 'cadInteraction')) {
        result.status = 'complete';
        break;
      }
      if (child.exitCode !== null || child.signalCode !== null) {
        result.status = 'exited-before-completion';
        result.exitCode = child.exitCode;
        break;
      }
      await delay(20);
    }
    result.status ??= 'timeout';
    result.observedWallMs = performance.now() - started;
    const feedback = phase(result.events, 'feedback');
    const workspace = phase(result.events, 'workspace');
    const interaction = phase(result.events, 'interaction');
    const cadInteraction = phase(result.events, 'cadInteraction');
    const fileReady = phase(result.events, 'fileReady');
    const origin = feedback?.processStartedUnixMs;
    result.beforeMainMs = Number.isFinite(origin) ? Math.max(0, origin - launchUnixMs) : null;
    const launchElapsed = (event) => event && result.beforeMainMs !== null ? result.beforeMainMs + event.processElapsedMs : null;
    result.firstFeedbackMs = launchElapsed(feedback);
    result.usableWorkspaceMs = launchElapsed(workspace);
    result.fileReadyMs = launchElapsed(fileReady);
    result.fileOpenMs = fileReady?.durationMs ?? null;
    const interactionStarted = phase(result.events, 'interaction-start');
    result.firstInteractionMs = interaction?.durationMs ?? (interactionStarted && interaction ? interaction.processElapsedMs - interactionStarted.processElapsedMs : null);
    result.firstCadInteractionMs = cadInteraction?.durationMs ?? null;
    try { result.cadTimings = cadInteraction?.detail ? JSON.parse(cadInteraction.detail) : null; }
    catch { result.cadTimings = null; }
    result.repeatedCadInteractionMs = result.cadTimings?.repeatedCadInteractionMs ?? null;
    if (result.status === 'complete' && (!feedback || !workspace || result.beforeMainMs === null || !Number.isFinite(result.firstInteractionMs) || result.firstInteractionMs < 0 || !Number.isFinite(result.firstCadInteractionMs) || result.firstCadInteractionMs < 0 || !Number.isFinite(result.repeatedCadInteractionMs) || result.repeatedCadInteractionMs < 0 || result.events.some((event) => event.pid !== child.pid) || interaction.processElapsedMs < workspace.processElapsedMs || cadInteraction.processElapsedMs < interaction.processElapsedMs || workspace.processElapsedMs < feedback.processElapsedMs)) {
      result.status = 'invalid-milestones';
    }
    if (result.status === 'complete' && documentMode === 'runtime' && (!fileReady
      || !Number.isFinite(result.fileOpenMs) || result.fileOpenMs < 0
      || fileReady.processElapsedMs < workspace.processElapsedMs
      || fileReady.processElapsedMs > interaction.processElapsedMs)) {
      result.status = 'invalid-file-milestones';
    }
  } finally {
    // Only this exact child is retired. Do not match process names or kill any
    // installed/running CAD instance belonging to the user or another agent.
    if (child.exitCode === null && child.signalCode === null && child.pid) child.kill();
    await closed;
    await stdout.close();
    await stderr.close();
    (isWarmup ? metadata.warmups : metadata.runs).push(result);
    await fs.writeFile(path.join(directory, 'results.json'), JSON.stringify(metadata, null, 2));
  }
  console.log(JSON.stringify({ index, warmup: isWarmup, status: result.status, firstFeedbackMs: result.firstFeedbackMs, usableWorkspaceMs: result.usableWorkspaceMs, fileOpenMs: result.fileOpenMs, fileReadyMs: result.fileReadyMs, firstInteractionMs: result.firstInteractionMs, firstCadInteractionMs: result.firstCadInteractionMs, repeatedCadInteractionMs: result.repeatedCadInteractionMs, error: result.error }));
  if (result.status !== 'complete') break;
  await delay(250);
}
if (await sha256(executable) !== executableHash) throw new Error('Executable changed during measurement');
if (documentPath && await sha256(documentPath) !== metadata.documentSha256) throw new Error('Document changed during measurement');
metadata.inputVerification.status = 'verified';
metadata.inputVerification.verifiedAt = new Date().toISOString();
await fs.writeFile(path.join(directory, 'results.json'), JSON.stringify(metadata, null, 2));
if (metadata.warmups.some((run) => run.status !== 'complete') || metadata.runs.length !== runs || metadata.runs.some((run) => run.status !== 'complete')) process.exitCode = 1;
