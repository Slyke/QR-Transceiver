<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { emit, listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { open } from '@tauri-apps/plugin-dialog';
  import { rpc, native, bytes, duration } from './lib/api';
  import { t } from './lib/i18n';
  import QrDisplay from './lib/QrDisplay.svelte';
  import HexViewer from './lib/HexViewer.svelte';
  import Settings from './lib/Settings.svelte';
  import SendControls from './lib/SendControls.svelte';
  import SectionOrder from './lib/SectionOrder.svelte';
  import { createSectionOrder } from './lib/section-order.svelte';
  import Automation from './lib/Automation.svelte';
  import type { Status, Job, Mode, DisplayFrame, TransferView, TransferEvent, CameraStatus, Profile, DisplaySettings } from './lib/types';
  type Section = 'dashboard' | 'send' | 'receive' | 'history' | 'automation' | 'settings';
  let section = $state<Section>('dashboard');
  const layouts = createSectionOrder({
    dashboard: ['outgoing', 'incoming', 'queue', 'payload'],
    send: ['outgoing', 'queue', 'payload'],
    receive: ['incoming', 'queue', 'payload']
  });
  const sectionIds = $derived(
    layouts
      .get(section)
      .filter((id) => id !== 'outgoing' || showSend)
      .filter((id) => id !== 'incoming' || showReceive)
  );
  const moveSection = ({ id, direction }: { id: string; direction: number }) =>
    layouts.move({ page: section, id, direction, visible: sectionIds });
  let status = $state<Status | null>(null);
  let jobs = $state<Job[]>([]);
  let selected = $state('');
  let selectedFile = $state(0);
  let view = $state<TransferView | null>(null);
  let current = $state<DisplayFrame | null>(null);
  let displaying = $state(false);
  let error = $state('');
  let path = $state('');
  let recursive = $state(true);
  let busy = $state(false);
  let theme = $state('dark');
  let revision = $state(0);
  let digest = $state('');
  let events = $state<[number, TransferEvent][]>([]);
  let automation = $state<{
    webhooks: { id: string; enabled: boolean; events: string[] }[];
    commands: { id: string; enabled: boolean; events: string[] }[];
    results: { rule: string; status: string; error: string | null }[];
  } | null>(null);
  let fps = $state(0);
  let camera = $state<CameraStatus | null>(null);
  let nextAt = 0;
  let displayRevision = 0;
  let presentationBusy = false;
  let settingsQueue = Promise.resolve();
  const channel = !native ? new BroadcastChannel('qrtx-display') : null;
  const publish = async () => {
    if (native) await emit('qrtx-display', current);
    else channel?.postMessage({ frame: $state.snapshot(current) });
  };
  const changeDisplay = (settings: DisplaySettings) => {
    settingsQueue = settingsQueue
      .then(async () => {
        presentationBusy = true;
        displayRevision += 1;
        try {
          const profile = await rpc<Profile>({ method: 'display_settings', params: settings });
          if (status) status = { ...status, profile };
          if (current) current = await rpc<DisplayFrame>({ method: 'redraw_frame', params: { text: current.text, cycle: current.cycle } });
          nextAt = performance.now() + profile.frame_ms;
          await publish();
        } finally {
          presentationBusy = false;
        }
      })
      .catch(report);
  };
  let displayedTimes: number[] = [];
  let schedulerBusy = false;
  let digestBusy = false;
  const detached = new URLSearchParams(location.search).has('display');
  const activeJob = $derived(jobs.find((job) => job.id === selected) ?? null);
  const activeFile = $derived(activeJob?.manifest.files.find((file) => file.id === selectedFile && !file.directory) ?? null);
  const sending = $derived(jobs.filter((job) => job.direction === 'send'));
  const receiving = $derived(jobs.filter((job) => job.direction === 'receive'));
  const mode = $derived(status?.mode ?? 'both');
  const showSend = $derived(section === 'send' || (section === 'dashboard' && mode !== 'receive'));
  const showReceive = $derived(section === 'receive' || (section === 'dashboard' && mode !== 'send'));
  const report = (e: unknown) => {
    error = e instanceof Error ? e.message : String(e);
  };
  const run = async (action: () => Promise<void>) => {
    try {
      error = '';
      await action();
    } catch (e) {
      report(e);
    }
  };
  const refresh = async () => {
    const [s, j] = await Promise.all([rpc<Status>({ method: 'status' }), rpc<Job[]>({ method: 'transfers' })]);
    status = s;
    jobs = j;
    if ((!selected || !jobs.some((job) => job.id === selected)) && jobs.length) choose({ job: jobs[0] });
    if (!jobs.length) {
      selected = '';
      view = null;
    }
    if (selected && jobs.some((j) => j.id === selected)) view = await rpc<TransferView>({ method: 'transfer', params: { id: selected } });
    revision += 1;
    const receiving = jobs.find((j) => j.id === selected && j.direction === 'receive');
    if (receiving && selectedFile && !digestBusy) {
      digestBusy = true;
      const id = selected;
      const file = selectedFile;
      void rpc<string>({ method: 'digest', params: { id, file } })
        .then((value) => {
          if (selected === id && selectedFile === file) digest = value;
        })
        .catch(report)
        .finally(() => (digestBusy = false));
    }
  };
  const choose = ({ job }: { job: Job }) => {
    selected = job.id;
    selectedFile = job.manifest.files.find((f) => !f.directory)?.id ?? 0;
    digest = '';
    void run(async () => {
      view = await rpc<TransferView>({ method: 'transfer', params: { id: selected } });
    });
  };
  const enqueue = async ({ paths, directory = false }: { paths: string[]; directory?: boolean }) => {
    busy = true;
    try {
      const job = await rpc<Job>({ method: 'enqueue', params: { paths, recursive: directory && recursive } });
      await refresh();
      choose({ job });
      path = '';
    } finally {
      busy = false;
    }
  };
  const selectFiles = async ({ directory }: { directory: boolean }) => {
    if (!native) {
      document.getElementById('manual-path')?.focus();
      return;
    }
    const result = await open({ directory, multiple: !directory });
    if (result) await enqueue({ paths: Array.isArray(result) ? result : [result], directory });
  };
  const jobAction = async ({ id, action, priority }: { id: string; action: string; priority?: number }) => {
    await rpc({ method: 'action', params: { id, action, priority } });
    await refresh();
  };
  const updateFrame = async () => {
    if (!displaying || schedulerBusy || presentationBusy || detached) return;
    schedulerBusy = true;
    try {
      const revision = displayRevision;
      const frame = await rpc<DisplayFrame | null>({ method: 'next_frame' });
      if (revision !== displayRevision) return;
      current = frame;
      nextAt = performance.now() + (frame?.duration_ms ?? status?.profile.frame_ms ?? 250);
      await publish();
      if (frame) {
        jobs = jobs.map((j) => (j.id === frame.frame.transfer ? { ...j, last_frame: frame.frame } : j));
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        displayedTimes.push(performance.now());
        displayedTimes = displayedTimes.slice(-32);
        if (displayedTimes.length > 1) fps = ((displayedTimes.length - 1) * 1000) / (displayedTimes.at(-1)! - displayedTimes[0]);
      }
    } catch (e) {
      report(e);
      displaying = false;
    } finally {
      schedulerBusy = false;
    }
  };
  const fullscreen = async () => {
    if (native) {
      const win = getCurrentWindow();
      await win.setFullscreen(!(await win.isFullscreen()));
    } else if (!document.fullscreenElement) await document.documentElement.requestFullscreen();
    else await document.exitFullscreen();
  };
  const toggleTheme = () => {
    theme = theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem('qrtx-theme', theme);
    } catch {
      /* theme stays usable */
    }
  };
  const navigate = async ({ next }: { next: Section }) => {
    section = next;
    if (next === 'automation') automation = await rpc({ method: 'automation' });
    if (next === 'history') events = await rpc({ method: 'events', params: { limit: 200, history: true } });
  };
  onMount(() => {
    try {
      theme = localStorage.getItem('qrtx-theme') === 'light' ? 'light' : 'dark';
    } catch {
      /* use default */
    }
    document.documentElement.dataset.theme = theme;
    try {
      const scale = Number(localStorage.getItem('qrtx-scale'));
      if (!detached && scale >= 80 && scale <= 150) document.documentElement.style.zoom = String(scale / 100);
    } catch {
      /* use default scale */
    }
    const keyboard = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'o') {
        event.preventDefault();
        void run(() => selectFiles({ directory: event.shiftKey }));
      }
      if (
        event.key === ' ' &&
        !(event.target instanceof HTMLInputElement) &&
        !(event.target instanceof HTMLTextAreaElement) &&
        !(event.target instanceof HTMLSelectElement) &&
        !(event.target instanceof HTMLButtonElement)
      ) {
        event.preventDefault();
        displaying = !displaying;
      }
    };
    if (!detached) window.addEventListener('keydown', keyboard);
    if (detached && native) {
      const subscription = listen<DisplayFrame | null>('qrtx-display', (event) => (current = event.payload));
      void subscription.then(() => emit('qrtx-display-ready'));
      return () => {
        void subscription.then((unlisten) => unlisten()).catch(report);
      };
    }
    if (channel) {
      channel.onmessage = (event) => {
        if (detached && 'frame' in event.data) current = event.data.frame;
        if (!detached && event.data.ready) void publish();
      };
      if (detached) {
        channel.postMessage({ ready: true });
        return () => channel.close();
      }
    }
    const ready = native ? listen('qrtx-display-ready', () => void publish()) : null;
    void run(refresh);
    let alive = true;
    let cameraTimer: ReturnType<typeof setTimeout>;
    const preview = async () => {
      if (!alive) return;
      try {
        camera = await rpc<CameraStatus>({ method: 'camera' });
      } catch (e) {
        report(e);
      }
      if (alive) cameraTimer = setTimeout(() => void preview(), showReceive && camera?.running ? 80 : 500);
    };
    void preview();
    const poll = async () => {
      if (!alive) return;
      await refresh().catch(report);
      if (alive) timer = setTimeout(() => void poll(), 1000);
    };
    let timer = setTimeout(() => void poll(), 1000);
    const displayTimer = setInterval(() => {
      const now = performance.now();
      if (now >= nextAt) {
        void updateFrame();
      }
    }, 20);
    return () => {
      alive = false;
      clearTimeout(timer);
      clearTimeout(cameraTimer);
      channel?.close();
      void ready?.then((unlisten) => unlisten());
      clearInterval(displayTimer);
      window.removeEventListener('keydown', keyboard);
    };
  });
</script>

{#if detached}
  <main class="detached">
    {#if current}<div class="detached-qr"><QrDisplay svg={current.svg} fit /></div>
      <p>{current.frame.kind} · {current.frame.index + 1}/{current.frame.total}</p>{:else}<p>{t('qrtx-idle_display')}</p>{/if}<button
      onclick={() => void run(fullscreen)}>{t('qrtx-fullscreen')}</button
    >
  </main>
{:else}
  <div class="app-shell">
    <aside class="sidebar">
      <a class="brand" href="#workspace"
        ><span class="brand-icon"><i></i><i></i><i></i><b></b></span><span
          >QR<span class="brand-light">TX</span><small>TRANSCEIVER</small></span
        ></a
      >
      <div class="sidebar-label">WORKSPACE</div>
      <nav aria-label="Main navigation">
        {#each [['dashboard', '◫'], ['send', '↗'], ['receive', '↙'], ['history', '◷'], ['automation', '⌘'], ['settings', '⚙']] as [name, icon]}
          <button class:current={section === name} onclick={() => void run(() => navigate({ next: name as Section }))}
            ><span>{icon}</span>{t(('qrtx-' + name) as Parameters<typeof t>[0])}{#if name === 'send' && sending.length}<b
                >{sending.length}</b
              >{/if}</button
          >
        {/each}
      </nav>
      <div class="sidebar-bottom">
        <div class="version">QRTX / 1 <span>v{status?.version ?? '0.1.0'}</span></div>
      </div>
    </aside>
    <div class="workspace">
      <header class="topbar">
        <div class="breadcrumb">QR TRANSCEIVER <span>/</span>{t(('qrtx-' + section) as Parameters<typeof t>[0])}</div>
        <div class="topbar-right">
          <span class="connection" class:disconnected={!status}><i></i>{status ? 'Local engine' : 'Connecting'}</span><button
            class="icon-button"
            aria-label="Toggle theme"
            title={theme === 'dark' ? t('qrtx-light_theme') : t('qrtx-dark_theme')}
            onclick={toggleTheme}
          >
            <svg
              class="theme-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.7"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              {#if theme === 'dark'}<circle cx="12" cy="12" r="4" /><path
                  d="M12 2v2m0 16v2M2 12h2m16 0h2M4.93 4.93l1.42 1.42m11.3 11.3 1.42 1.42M4.93 19.07l1.42-1.42m11.3-11.3 1.42-1.42"
                />
              {:else}<path d="M20.9 13a9 9 0 0 1-9.9-9.9A9 9 0 1 0 20.9 13Z" />{/if}
            </svg></button
          >
        </div>
      </header>
      <main id="workspace">
        {#if error}<div class="error banner" role="alert">
            <span>{error}</span><button aria-label="Dismiss error" onclick={() => (error = '')}>×</button>
          </div>{/if}
        <div class="page-heading">
          <div>
            <div class="eyebrow">{t('qrtx-tagline')}</div>
            <h1>{section === 'dashboard' ? t('qrtx-workspace') : t(('qrtx-' + section) as Parameters<typeof t>[0])}</h1>
            <p>
              {section === 'settings'
                ? t('qrtx-settings_description')
                : section === 'automation'
                  ? t('qrtx-automation_description')
                  : t('qrtx-workspace_description')}
            </p>
          </div>
          {#if ['dashboard', 'send', 'receive'].includes(section)}<div class="mode-switch" aria-label="Operating mode">
              {#each ['send', 'receive', 'both'] as item}<button
                  class:chosen={mode === item}
                  aria-pressed={mode === item}
                  onclick={() =>
                    void run(async () => {
                      if (status) {
                        await rpc({ method: 'set_profile', params: { ...status.profile, mode: item as Mode } });
                        await refresh();
                      }
                    })}>{t(('qrtx-' + item) as Parameters<typeof t>[0])}</button
                >{/each}
            </div>{/if}
        </div>
        {#if ['dashboard', 'send', 'receive'].includes(section)}
          <div class="stats-strip">
            <div>
              <span>{t('qrtx-queue')}</span><strong
                >{sending
                  .filter((j) => j.status === 'active')
                  .length.toString()
                  .padStart(2, '0')}<small> ACTIVE</small></strong
              >
            </div>
            <div>
              <span>{showReceive && !showSend ? t('qrtx-capture_rate') : t('qrtx-frame_rate')}</span><strong
                >{showReceive && !showSend ? (camera?.capture_fps ?? 0).toFixed(1) : displaying ? fps.toFixed(1) : '0.0'}<small>
                  FPS</small
                ></strong
              >
            </div>
            <div><span>RECONSTRUCTED</span><strong>{bytes({ value: receiving.reduce((sum, j) => sum + j.bytes, 0) })}</strong></div>
            <div>
              <span>VERIFIED FILES</span><strong
                >{receiving
                  .reduce((sum, j) => sum + j.delivered_files.length, 0)
                  .toString()
                  .padStart(2, '0')}<small class="good"> SHA-256</small></strong
              >
            </div>
          </div>
          {#snippet outgoingPanel()}
            <section data-section="outgoing" class="panel transport-panel">
              <div class="panel-heading">
                <h2><span class="direction">↗</span>{t('qrtx-outgoing')}</h2>
                <SectionOrder
                  id="outgoing"
                  title="Outgoing"
                  order={sectionIds}
                  move={(direction) => moveSection({ id: 'outgoing', direction })}
                />
                <span class="badge" class:live={displaying}>{displaying ? 'TRANSMITTING' : 'STANDBY'}</span>
              </div>
              <div class="qr-area">
                {#if current}<div class="qr-image"><QrDisplay svg={current.svg} /></div>{:else}<div class="empty-display">
                    <div class="scan-glyph"><i></i><i></i><i></i><i></i><span>QR</span></div>
                    <h3>{t('qrtx-idle_display')}</h3>
                    <p>{t('qrtx-idle_display_detail')}</p>
                  </div>{/if}
              </div>
              <div class="frame-info">
                <span
                  >{current
                    ? current.frame.kind.toUpperCase() + ' · ' + (current.frame.index + 1) + ' / ' + current.frame.total
                    : 'QRTX/1 · BASE45'}</span
                ><span>{current ? 'CRC32C ' + current.checksum : 'OPTICAL TRANSPORT'}</span>
              </div>
              <div class="panel-actions">
                <button class="primary" onclick={() => (displaying = !displaying)}
                  >{displaying ? 'Ⅱ ' + t('qrtx-pause') : '▷ ' + t('qrtx-start')}</button
                ><button
                  onclick={() =>
                    void run(async () => {
                      if (native) await invoke('detach');
                      else window.open('?display', 'qrtx-display', 'width=900,height=900');
                    })}>{t('qrtx-detach')} ↗</button
                >
              </div>
              <SendControls profile={status?.profile} change={changeDisplay} />
              <p class="footnote">{t('qrtx-once_note')}</p>
            </section>
          {/snippet}
          {#snippet incomingPanel()}
            <section data-section="incoming" class="panel transport-panel">
              <div class="panel-heading">
                <h2><span class="direction incoming">↙</span>{t('qrtx-incoming')}</h2>
                <SectionOrder
                  id="incoming"
                  title="Incoming"
                  order={sectionIds}
                  move={(direction) => moveSection({ id: 'incoming', direction })}
                />
                <span class="badge" class:live={camera?.running}>{camera?.running ? 'SCANNING' : 'CAMERA IDLE'}</span>
              </div>
              <div class="camera-area">
                {#if camera?.image}<img src={camera.image} alt="Live USB-camera preview" />
                  <div class="camera-target"></div>{:else}<div class="empty-display">
                    <div class="camera-glyph"><span></span></div>
                    <h3>{t('qrtx-waiting_camera')}</h3>
                    <p>{camera?.error ?? t('qrtx-waiting_camera_detail')}</p>
                  </div>{/if}
              </div>
              <div class="frame-info">
                <span
                  >{camera?.running
                    ? `${(camera.capture_fps ?? 0).toFixed(1)} FPS CAPTURE · ${(camera.scan_fps ?? 0).toFixed(1)} FPS SCAN`
                    : 'USB WEBCAM'}</span
                ><span>{camera?.decoded ?? 0} VALID FRAMES</span>
              </div>
              <div class="panel-actions">
                <button
                  class="secondary"
                  onclick={() =>
                    void run(async () => {
                      await rpc({ method: camera?.running ? 'receive_stop' : 'receive_start' });
                      await refresh();
                    })}>{camera?.running ? '■ ' + t('qrtx-stop_camera') : '◎ ' + t('qrtx-start_camera')}</button
                ><span class="muted">{status?.profile.camera_width ?? 1280} × {status?.profile.camera_height ?? 720}</span>
              </div>
              <p class="footnote">{t('qrtx-download_verified')}</p>
            </section>
          {/snippet}
          {#snippet queuePanel()}
            <section data-section="queue" class="panel queue-panel">
              <div class="panel-heading">
                <h2>{t('qrtx-queue')} <span class="count">{jobs.length}</span></h2>
                <SectionOrder
                  id="queue"
                  title="Transmission queue"
                  order={sectionIds}
                  move={(direction) => moveSection({ id: 'queue', direction })}
                />
                <div class="button-group">
                  <button disabled={busy} onclick={() => void run(() => selectFiles({ directory: false }))}>+ {t('qrtx-add_files')}</button
                  ><button disabled={busy} onclick={() => void run(() => selectFiles({ directory: true }))}
                    >+ {t('qrtx-add_directory')}</button
                  >
                </div>
              </div>
              <div class="path-entry">
                <input
                  id="manual-path"
                  aria-label="File or directory path"
                  bind:value={path}
                  placeholder="Local file or directory path…"
                /><button disabled={busy || !path.trim()} onclick={() => void run(() => enqueue({ paths: [path], directory: true }))}
                  >{busy ? 'Snapshotting…' : t('qrtx-enqueue')}</button
                ><label class="inline"><input type="checkbox" bind:checked={recursive} />{t('qrtx-recursive')}</label>
              </div>
              {#if !jobs.length}<div class="empty-queue">
                  <span>＋</span>
                  <div>
                    <h3>{t('qrtx-no_transfers')}</h3>
                    <p>{t('qrtx-no_transfers_detail')}</p>
                  </div>
                </div>
              {:else}<div class="queue-table">
                  <table>
                    <thead><tr><th>TRANSFER / FILES</th><th>SIZE</th><th>STATUS</th><th>CYCLE</th><th>ACTIONS</th></tr></thead><tbody>
                      {#each jobs.filter( (j) => (section === 'send' ? j.direction === 'send' : section === 'receive' ? j.direction === 'receive' : true) ) as job}
                        <tr class:selected-row={selected === job.id}
                          ><td
                            ><button class="file-select" onclick={() => choose({ job })}
                              ><span class="file-symbol">{job.direction === 'send' ? '↗' : '↙'}</span><span
                                >{job.manifest.files.find((f) => !f.directory)?.path ?? job.manifest.files[0]?.path}<small
                                  >{job.manifest.files.length} entries · {job.id.slice(0, 8)}</small
                                ></span
                              ></button
                            ></td
                          ><td>{bytes({ value: job.manifest.files.reduce((sum, f) => sum + f.size, 0) })}</td><td
                            ><span class="badge" class:live={['complete', 'confirmed'].includes(job.status)}>{job.status}</span></td
                          ><td>{job.direction === 'send' ? job.cycle : '—'}</td><td
                            ><div class="button-group compact-actions">
                              <button
                                aria-label="Increase transfer priority"
                                onclick={() => void run(() => jobAction({ id: job.id, action: 'priority', priority: job.priority + 1 }))}
                                >↑</button
                              ><button
                                aria-label="Pause or resume transfer"
                                disabled={['complete', 'confirmed', 'cancelled'].includes(job.status)}
                                onclick={() =>
                                  void run(() => jobAction({ id: job.id, action: job.status === 'paused' ? 'resume' : 'pause' }))}
                                >{job.status === 'paused' ? '▷' : 'Ⅱ'}</button
                              ><button
                                aria-label="Lower transfer priority"
                                onclick={() => void run(() => jobAction({ id: job.id, action: 'priority', priority: job.priority - 1 }))}
                                >↓</button
                              >{#if job.direction === 'send'}<button
                                  aria-label="Restart transfer"
                                  onclick={() => void run(() => jobAction({ id: job.id, action: 'restart' }))}>↻</button
                                >{/if}{#if job.status !== 'active'}<button
                                  aria-label="Remove transfer from queue"
                                  onclick={() => void run(() => jobAction({ id: job.id, action: 'remove' }))}>Remove</button
                                >{/if}<button
                                class="danger-ghost"
                                aria-label="Cancel transfer"
                                onclick={() => void run(() => jobAction({ id: job.id, action: 'cancel' }))}>×</button
                              >
                            </div></td
                          ></tr
                        >
                      {/each}
                    </tbody>
                  </table>
                </div>{/if}
            </section>
          {/snippet}
          {#snippet payloadPanel()}
            <section data-section="payload" class="panel payload-panel">
              <div class="panel-heading">
                <h2>{t('qrtx-payload')}</h2>
                <SectionOrder
                  id="payload"
                  title="Payload inspector"
                  order={sectionIds}
                  move={(direction) => moveSection({ id: 'payload', direction })}
                />
                {#if activeJob}<select aria-label="Inspect file" bind:value={selectedFile}
                    >{#each activeJob.manifest.files.filter((f) => !f.directory) as file}<option value={file.id}>{file.path}</option
                      >{/each}</select
                  >{:else}<span class="muted">HEX / UTF-8</span>{/if}
              </div>
              {#if activeJob && activeFile}
                {#key activeJob.id + ':' + activeFile.id}<HexViewer job={activeJob} file={activeFile} {revision} />{/key}
                <div class="integrity-grid">
                  <div>
                    <div class="field-label">{t('qrtx-expected_hash')}</div>
                    <code>{activeFile.sha256}</code>
                    <div class="field-label">{t('qrtx-manifest_hash')}</div>
                    <code>{activeJob.manifest_hash}</code>
                  </div>
                  <div>
                    <div class="field-label">{t('qrtx-integrity')}</div>
                    <strong class:good={activeJob.delivered_files.includes(activeFile.id)}
                      >{activeJob.delivered_files.includes(activeFile.id)
                        ? activeJob.direction === 'receive'
                          ? 'VERIFIED & COMMITTED'
                          : activeJob.confirmed_files.includes(activeFile.id)
                            ? 'DELIVERY CONFIRMED'
                            : 'PEER ACKNOWLEDGED'
                        : 'AWAITING COMPLETION'}</strong
                    >
                    <p>{activeJob.duplicates} duplicates · {activeJob.rejected} rejected</p>
                    <p>
                      {view?.files.find((f) => f.file === activeFile.id)?.received_chunks ?? 0} acknowledged / received chunks · {activeFile.chunks}
                      total
                    </p>
                    <p>
                      {activeJob.frames} frames · {bytes({ value: activeJob.bytes })} processed · elapsed {duration({
                        seconds: (Date.now() - activeJob.created) / 1000
                      })}
                    </p>
                    {#if activeJob.last_frame}<p>
                        Latest {activeJob.last_frame.kind.toLowerCase()} frame · offset {activeJob.last_frame.offset} · {new Date(
                          activeJob.last_activity
                        ).toLocaleTimeString()}
                      </p>{/if}{#if activeJob.error}<p class="error">
                        {activeJob.error}
                      </p>{/if}{#if view?.files.find((f) => f.file === activeFile.id)?.path}<code
                        >{view.files.find((f) => f.file === activeFile.id)?.path}</code
                      >{/if}
                    <p>{t('qrtx-loop_time')}: {duration({ seconds: view?.estimated_loop_seconds ?? null })}</p>
                    {#if (view?.estimated_loop_seconds ?? 0) > 3600}<p class="warning">
                        This transfer requires a long optical session.
                      </p>{/if}
                  </div>
                </div>
                {#if activeJob.direction === 'receive'}{#if native && !activeJob.delivered_files.length}<button
                      onclick={() =>
                        void run(async () => {
                          const output = await open({ directory: true, multiple: false });
                          if (typeof output === 'string') {
                            await rpc({ method: 'destination', params: { id: activeJob.id, path: output } });
                            await refresh();
                          }
                        })}>Choose this transfer's destination</button
                    >{/if}
                  <div class="receive-progress">
                    <progress
                      max={activeFile.size || 1}
                      value={activeJob.delivered_files.includes(activeFile.id)
                        ? activeFile.size || 1
                        : (view?.files.find((f) => f.file === activeFile.id)?.received_bytes ?? 0)}
                    ></progress><span
                      >{t('qrtx-useful_rate')}: {bytes({ value: view?.useful_bytes_per_second ?? 0 })}/s · {t('qrtx-eta')}: {duration({
                        seconds: view?.eta_seconds ?? null
                      })}</span
                    >
                  </div>
                  <div class="panel-actions">
                    <button
                      onclick={() =>
                        void run(async () => {
                          await rpc({ method: 'missing', params: { id: selected, file: selectedFile } });
                        })}>{t('qrtx-next_missing')}</button
                    ><button
                      onclick={() =>
                        void run(async () => {
                          digest = await rpc<string>({ method: 'digest', params: { id: selected, file: selectedFile } });
                        })}>{t('qrtx-recalculate')}</button
                    >
                  </div>
                  {#if digest}<div class="field-label">{t('qrtx-received_digest')}</div>
                    <code class="digest">{digest}</code>{/if}{/if}
              {:else}<div class="empty-inspector">
                  <span>00 00 00 00 <b>-- -- -- --</b></span>
                  <p>{t('qrtx-payload_empty')}</p>
                </div>{/if}
            </section>
          {/snippet}
          <div class="ordered-sections transfer-sections" class:single={!showSend || !showReceive}>
            {#each sectionIds as id (id)}
              {#if id === 'outgoing'}{@render outgoingPanel()}{:else if id === 'incoming'}{@render incomingPanel()}{:else if id === 'queue'}{@render queuePanel()}{:else}{@render payloadPanel()}{/if}
            {/each}
          </div>
        {:else if section === 'history'}
          <section class="panel">
            <div class="panel-heading">
              <h2>{t('qrtx-events')}</h2>
              <button
                onclick={() =>
                  void run(async () => {
                    events = await rpc({ method: 'events', params: { limit: 200, history: true } });
                  })}>Refresh</button
              >
            </div>
            {#if !events.length}<p class="empty-queue">{t('qrtx-empty_history')}</p>{:else}<div class="queue-table">
                <table>
                  <thead><tr><th>TIME</th><th>EVENT</th><th>TRANSFER</th><th>DETAILS</th></tr></thead><tbody
                    >{#each [...events].reverse() as [, event]}<tr
                        ><td>{new Date(event.timestamp).toLocaleTimeString()}</td><td>{event.event}</td><td>{event.transfer.slice(0, 8)}</td
                        ><td
                          ><details>
                            <summary>Inspect event</summary>
                            <pre>{JSON.stringify(event, null, 2)}</pre>
                          </details></td
                        ></tr
                      >{/each}</tbody
                  >
                </table>
              </div>{/if}
          </section>
        {:else if section === 'automation'}
          <Automation {report} />
        {:else if section === 'settings' && status}
          <Settings {status} {refresh} {report} />
        {/if}
        <footer><span>QRTX/1 · BYTE-PERFECT OPTICAL TRANSPORT</span><span>CRC32C / SHA-256 / ED25519</span></footer>
      </main>
    </div>
  </div>
{/if}
