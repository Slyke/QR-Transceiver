<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { rpc, bytes } from './api';
  import { t } from './i18n';
  import type { Job, FileEntry, PayloadPage } from './types';
  let { job, file, revision = 0 }: { job: Job; file: FileEntry; revision?: number } = $props();
  let page = $state<PayloadPage | null>(null);
  let offset = $state(0);
  let jump = $state('0');
  let query = $state('');
  let textMode = $state(false);
  let encoding = $state('utf-8');
  let follow = $state(true);
  let error = $state('');
  let notice = $state('');
  let selection = $state<[number, number] | null>(null);
  let viewport: HTMLDivElement;
  let host: HTMLDivElement;
  let bytesPerRow = $state(16);
  let rowChoice = $state('auto');
  let showAscii = $state(true);
  let expectedScroll: number | null = null;
  const rowChoices = [16, 32, 48, 64, 80, 96, 112, 128];
  const layout = ({ persist = false } = {}) => {
    if (persist) {
      try {
        localStorage.setItem('qrtx-hex-layout', JSON.stringify({ rows: rowChoice, ascii: showAscii }));
      } catch {
        /* Keep session preference. */
      }
    }
    const columns =
      rowChoice === 'auto'
        ? Math.max(16, Math.min(128, Math.floor((host.clientWidth - 128) / (showAscii ? 22 : 16) / 16) * 16))
        : Number(rowChoice);
    if (columns !== bytesPerRow) {
      const at = offset;
      bytesPerRow = columns;
      go({ at });
    }
  };
  let scrollTop = $state(0);
  let map = $state<number[]>([]);
  const identity = $derived(job.id + ':' + file.id);
  let request = 0;
  const rowHeight = 26;
  const rowCount = 32;
  const visibleRows = 14;
  const logicalRows = $derived(Math.max(1, Math.ceil(file.size / bytesPerRow)));
  const height = $derived(Math.min(logicalRows * rowHeight, 8_000_000));
  const renderTop = $derived(
    logicalRows * rowHeight <= 8_000_000
      ? (offset / bytesPerRow) * rowHeight
      : Math.min(scrollTop, Math.max(0, height - Math.ceil((page?.bytes.length ?? 0) / bytesPerRow) * rowHeight))
  );
  const rows = $derived(
    Array.from({ length: Math.ceil((page?.bytes.length ?? 0) / bytesPerRow) }, (_, i) => ({
      address: (page?.offset ?? 0) + i * bytesPerRow,
      values: page?.bytes.slice(i * bytesPerRow, i * bytesPerRow + bytesPerRow) ?? []
    }))
  );
  const load = async ({ at }: { at: number }) => {
    const current = ++request;
    offset = at;
    try {
      const result = await rpc<PayloadPage>({
        method: 'payload',
        params: { id: job.id, file: file.id, offset: Math.min(at, file.size), length: rowCount * bytesPerRow }
      });
      if (current === request) {
        page = result;
        offset = result.offset;
        error = '';
      }
    } catch (e) {
      if (current === request) error = String(e);
    }
  };
  const go = ({ at }: { at: number }) => {
    const row = Math.max(0, Math.min(Math.floor(at / bytesPerRow), logicalRows - visibleRows));
    if (viewport) {
      viewport.scrollTop =
        logicalRows * rowHeight <= 8_000_000
          ? row * rowHeight
          : (row / Math.max(1, logicalRows - visibleRows)) * Math.max(0, height - viewport.clientHeight);
      expectedScroll = viewport.scrollTop;
      scrollTop = viewport.scrollTop;
    }
    void load({ at: row * bytesPerRow });
  };
  const scroll = () => {
    const top = viewport.scrollTop;
    if (expectedScroll === null || Math.abs(top - expectedScroll) > 1) follow = false;
    expectedScroll = null;
    scrollTop = top;
    const row =
      logicalRows * rowHeight <= 8_000_000
        ? Math.floor(top / rowHeight)
        : Math.floor((top / Math.max(1, height - viewport.clientHeight)) * Math.max(0, logicalRows - visibleRows));
    void load({ at: Math.max(0, row) * bytesPerRow });
  };
  const active = ({ at }: { at: number }) =>
    (job.last_frame?.kind === 'Data' || job.last_frame?.kind === 'Bundle') &&
    job.last_frame.file === file.id &&
    at >= job.last_frame.offset &&
    at < job.last_frame.offset + job.last_frame.payload.length;
  const select = ({ at, shift }: { at: number; shift: boolean }) => {
    selection = shift && selection ? [selection[0], at] : [at, at];
  };
  const selected = ({ at }: { at: number }) => selection && at >= Math.min(...selection) && at <= Math.max(...selection);
  const copy = async ({ text }: { text: boolean }) => {
    try {
      const start = selection ? Math.min(...selection) : offset;
      const end = selection ? Math.max(...selection) + 1 : offset + (page?.bytes.length ?? 0);
      if (end - start > 65536) throw new Error('Copy a selection of up to 64 KiB.');
      const p = await rpc<PayloadPage>({ method: 'payload', params: { id: job.id, file: file.id, offset: start, length: end - start } });
      if (p.bytes.some((b) => b === null)) throw new Error('Selection contains missing bytes.');
      const values = p.bytes as number[];
      await navigator.clipboard.writeText(
        text ? new TextDecoder(encoding).decode(new Uint8Array(values)) : values.map((b) => b.toString(16).padStart(2, '0')).join(' ')
      );
      notice = 'Copied';
    } catch (e) {
      error = String(e);
    }
  };
  const search = async () => {
    try {
      const found = await rpc<number | null>({ method: 'search', params: { id: job.id, file: file.id, offset, query, hex: !textMode } });
      notice = found === null ? 'No match in available bytes' : `Match at 0x${found.toString(16)}`;
      if (found !== null) {
        follow = false;
        go({ at: found });
      }
    } catch (e) {
      error = String(e);
    }
  };
  const textRow = ({ values }: { values: (number | null)[] }) =>
    values.map((b) => (b === null ? '▧' : b >= 32 && b <= 126 ? String.fromCharCode(b) : '·')).join('');
  const textRows = $derived.by(() => {
    const decoder = new TextDecoder(encoding, { fatal: false });
    return rows.map((row, index) => {
      let text = '';
      let run: number[] = [];
      const flush = ({ end = false } = {}) => {
        text += decoder.decode(new Uint8Array(run), { stream: !end });
        run = [];
      };
      for (const value of row.values) {
        if (value === null) {
          flush({ end: true });
          text += '▧';
        } else run.push(value);
      }
      flush({ end: index === rows.length - 1 });
      return {
        address: row.address,
        text: text
          .replace(/\r/g, '␍')
          .replace(/\n/g, '↵')
          .replace(/\t/g, '⇥')
          .replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '·')
      };
    });
  });
  $effect(() => {
    void identity;
    selection = null;
    untrack(() => go({ at: 0 }));
  });
  $effect(() => {
    void revision;
    if (follow && job.last_frame?.file === file.id && (job.last_frame.kind === 'Data' || job.last_frame.kind === 'Bundle'))
      go({ at: job.last_frame.offset });
    else untrack(() => void load({ at: offset }));
  });
  onMount(() => {
    try {
      const saved = JSON.parse(localStorage.getItem('qrtx-hex-layout') ?? '{}');
      if (saved.rows === 'auto' || rowChoices.includes(Number(saved.rows))) rowChoice = saved.rows;
      if (typeof saved.ascii === 'boolean') showAscii = saved.ascii;
    } catch {
      /* Use automatic layout. */
    }
    const resize = new ResizeObserver(() => layout());
    resize.observe(host);
    layout();
    const update = async () => {
      try {
        map = await rpc<number[]>({ method: 'coverage', params: { id: job.id, file: file.id, bins: 160 } });
      } catch (e) {
        error = String(e);
      }
    };
    void update();
    const timer = setInterval(() => void update(), 1800);
    return () => {
      clearInterval(timer);
      resize.disconnect();
    };
  });
</script>

<div
  class="inspector"
  class:no-ascii={!showAscii}
  class:text-mode={textMode}
  bind:this={host}
  style={`--row-bytes: ${bytesPerRow}; --ascii-cell: ${showAscii ? 6 : 0}px`}
>
  <div class="inspector-toolbar">
    <div class="segmented small">
      <button class:chosen={!textMode} onclick={() => (textMode = false)}>{t('qrtx-hex')}</button><button
        class:chosen={textMode}
        onclick={() => (textMode = true)}>{t('qrtx-text')}</button
      >
    </div>
    {#if textMode}<select aria-label="Text encoding" bind:value={encoding}
        ><option value="utf-8">UTF-8</option><option value="windows-1252">Windows-1252</option><option value="utf-16le">UTF-16 LE</option
        ><option value="utf-16be">UTF-16 BE</option></select
      >{/if}
    <label class="inline"><input type="checkbox" bind:checked={follow} />{t('qrtx-follow')}</label>
    <label class="inline"
      >{t('qrtx-bytes_per_row')}<select
        aria-label={t('qrtx-bytes_per_row')}
        value={rowChoice}
        onchange={(event) => {
          rowChoice = event.currentTarget.value;
          layout({ persist: true });
        }}
      >
        <option value="auto">{t('qrtx-auto')} ({bytesPerRow})</option>
        {#each rowChoices as amount}<option value={String(amount)}>{amount}</option>{/each}
      </select></label
    >
    {#if !textMode}<label class="inline"
        ><input
          type="checkbox"
          checked={showAscii}
          onchange={(event) => {
            showAscii = event.currentTarget.checked;
            layout({ persist: true });
          }}
        />{t('qrtx-ascii')}</label
      >{/if}
    <span class="muted">{bytes({ value: file.size })}</span>
  </div>
  <div class="coverage" title="Received byte coverage" aria-label="Received byte coverage">
    {#each map as amount}<span class:filled={amount >= 1} class:partial={amount > 0 && amount < 1}></span>{/each}
  </div>
  <div class="hex-scroll">
    <div class="hex-head">
      <span>OFFSET</span>
      {#if textMode}<span>{encoding.toUpperCase()} · missing bytes marked ▧</span>
      {:else}<span class="hex-values"
          >{#each Array.from({ length: bytesPerRow }, (_, index) => index) as index}<span
              >{index.toString(16).padStart(2, '0').toUpperCase()}</span
            >{/each}</span
        >{#if showAscii}<span>ASCII</span>{/if}{/if}
    </div>
    <div class="hex-viewport" bind:this={viewport} onscroll={scroll} role="grid" aria-label="File bytes" tabindex="0">
      <div style:height={height + 'px'} class="hex-spacer">
        <div class="hex-rows" style:transform={`translateY(${renderTop}px)`}>
          {#if textMode}{#each textRows as row}<div class="text-row">
                <span class="address">{row.address.toString(16).padStart(8, '0')}</span>
                <span class="decoded-text">{row.text}</span>
              </div>{/each}
          {:else}{#each rows as row}
              <div class="hex-row">
                <span class="address">{row.address.toString(16).padStart(8, '0')}</span><span class="hex-values">
                  {#each row.values as value, i}<button
                      tabindex="-1"
                      class:unavailable={value === null}
                      class:active-byte={active({ at: row.address + i })}
                      class:selected-byte={selected({ at: row.address + i })}
                      class:verified-byte={page?.verified}
                      aria-label={`Byte ${row.address + i}: ${value ?? 'missing'}`}
                      onclick={(event) => select({ at: row.address + i, shift: event.shiftKey })}
                      >{value === null ? '--' : value.toString(16).padStart(2, '0')}</button
                    >{/each}
                </span>{#if showAscii}<span class="ascii">{textRow({ values: row.values })}</span>{/if}
              </div>
            {/each}{/if}
          {#if file.size === 0}<p class="empty-file">Empty file · SHA-256 still verified</p>{/if}
        </div>
      </div>
    </div>
  </div>
  <div class="legend">
    <span><i class="missing-dot"></i>{t('qrtx-missing')}</span><span><i class="received-dot"></i>{t('qrtx-validated')}</span><span
      ><i class="active-dot"></i>{t('qrtx-active')}</span
    >{#if page?.verified}<span class="good">{t('qrtx-verified')}</span>{/if}
  </div>
  <div class="inspector-actions">
    <label>{t('qrtx-offset')}<input aria-label="Jump to byte offset" bind:value={jump} placeholder="0x0000" /></label>
    <button
      onclick={() => {
        follow = false;
        go({ at: Number(jump) || 0 });
      }}>{t('qrtx-go')}</button
    >
    <input aria-label="Search bytes" bind:value={query} placeholder={textMode ? 'Search text' : 'Search hex: 48 65 6c'} />
    <button onclick={() => void search()}>{t('qrtx-search')}</button>
    <button onclick={() => void copy({ text: false })}>{t('qrtx-copy_hex')}</button>
    <button onclick={() => void copy({ text: true })}>{t('qrtx-copy_text')}</button>
  </div>
  {#if error}<p class="error compact" role="alert">{error}</p>{/if}
  {#if notice}<p class="muted compact" aria-live="polite">{notice}</p>{/if}
</div>
