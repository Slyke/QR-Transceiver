<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { t } from './i18n';
  import SectionOrder from './SectionOrder.svelte';
  import { createSectionOrder } from './section-order.svelte';
  import { rpc, native } from './api';
  import { availableMonitors } from '@tauri-apps/api/window';
  import type { Status, Profile } from './types';
  let { status, refresh, report }: { status: Status; refresh: () => Promise<void>; report: (e: unknown) => void } = $props();
  let profile = $state<Profile>(JSON.parse(JSON.stringify(untrack(() => status.profile))));
  let monitors = $state<string[]>([]);
  let profiles = $state<Profile[]>([]);
  let devices = $state<{ index: string; name: string }[]>([]);
  let peer = $state('');
  let name = $state('');
  let saved = $state('');
  let controlName = $state('');
  let controlValue = $state('');
  let scale = $state(100);
  let mcp = $state({ enabled: false, port: 38476 });
  let mcpSaved = $state('');
  const tabs = [
    { id: 'qr', label: 'qrtx-tab_qr' },
    { id: 'feedback', label: 'qrtx-tab_feedback' },
    { id: 'files', label: 'qrtx-tab_files' },
    { id: 'camera', label: 'qrtx-tab_camera' },
    { id: 'peers', label: 'qrtx-tab_peers' },
    { id: 'mcp', label: 'qrtx-tab_mcp' },
    { id: 'interface', label: 'qrtx-tab_interface' }
  ] as const;
  const layouts = createSectionOrder({ settings: tabs.map((tab) => tab.id) });
  const tabOrder = $derived(layouts.get('settings'));
  const orderedTabs = $derived(tabOrder.map((id) => tabs.find((tab) => tab.id === id)!));
  let activeTab = $state<string>('qr');
  const tabKey = (event: KeyboardEvent) => {
    const focused = (event.currentTarget as HTMLButtonElement).id.replace('settings-tab-', '');
    const index = orderedTabs.findIndex((tab) => tab.id === focused);
    const next =
      event.key === 'ArrowRight'
        ? (index + 1) % tabs.length
        : event.key === 'ArrowLeft'
          ? (index + tabs.length - 1) % tabs.length
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? tabs.length - 1
              : -1;
    if (next < 0) return;
    event.preventDefault();
    activeTab = orderedTabs[next].id;
    document.getElementById('settings-tab-' + activeTab)?.focus();
  };
  const run = async (action: () => Promise<void>) => {
    try {
      saved = '';
      await action();
    } catch (e) {
      report(e);
    }
  };
  onMount(() => {
    void run(async () => {
      mcp = await rpc({ method: 'mcp_settings' });
      profiles = await rpc({ method: 'profiles' });
      devices = (await rpc<{ cameras: { index: string; name: string }[] }>({ method: 'devices' })).cameras;
      if (native) monitors = (await availableMonitors()).map((m) => m.name ?? 'Unnamed display');
    });
  });
</script>

<div class="settings-tabs" role="tablist" aria-label={t('qrtx-settings_sections')}>
  {#each orderedTabs as tab}<button
      role="tab"
      id={'settings-tab-' + tab.id}
      aria-controls={'settings-panel-' + tab.id}
      aria-selected={activeTab === tab.id}
      tabindex={activeTab === tab.id ? 0 : -1}
      onclick={() => (activeTab = tab.id)}
      onkeydown={tabKey}>{t(tab.label)}</button
    >{/each}
</div>
<div class="settings-content">
  {#if activeTab === 'qr'}
    <div class="panel" role="tabpanel" id="settings-panel-qr" aria-labelledby="settings-tab-qr" tabindex="0">
      <div class="panel-heading">
        <h2>QR & timing</h2>
        <SectionOrder
          id={activeTab}
          title={t(tabs.find((tab) => tab.id === activeTab)!.label)}
          order={tabOrder}
          move={(direction) => layouts.move({ page: 'settings', id: activeTab, direction })}
        />
      </div>
      <div class="form-grid">
        <label>Profile name<input bind:value={profile.name} /></label><label
          >Frame duration (ms)<input type="number" min="50" max="60000" bind:value={profile.frame_ms} /></label
        >
        <label>Payload bytes per frame<input type="number" min="64" max="1200" bind:value={profile.chunk_size} /></label><label
          >Error correction<select bind:value={profile.correction}
            >{#each ['L', 'M', 'Q', 'H'] as level}<option>{level}</option>{/each}</select
          ></label
        >
        <label>Maximum QR version<input type="number" min="1" max="40" bind:value={profile.max_version} /></label><label
          >QR display size (px)<input type="number" min="128" max="4096" bind:value={profile.qr_pixels} /></label
        >
        <label>Manifest interval (frames)<input type="number" min="1" max="1024" bind:value={profile.manifest_interval} /></label><label
          >Transmission policy<select bind:value={profile.policy}
            ><option value="once">Once</option><option value="loop">Loop until stopped</option><option value="until_acknowledged"
              >Loop until acknowledged</option
            ><option value="directory_cycle">Directory cycle</option></select
          ></label
        >
        <label>Module pixels (0 = fit size)<input type="number" min="0" max="32" bind:value={profile.module_pixels} /></label>
        <label
          >Target frame rate<input
            type="number"
            min=".017"
            max="20"
            step=".1"
            value={1000 / profile.frame_ms}
            onchange={(e) => (profile.frame_ms = Math.round(1000 / Number(e.currentTarget.value)))}
          /></label
        >
        <label>Control frame every N slots<input type="number" min="2" max="64" bind:value={profile.control_interval} /></label>
        <label
          >Detached display<select bind:value={profile.display_name}
            ><option value={null}>Window's current monitor</option>{#each monitors as monitor}<option value={monitor}>{monitor}</option
              >{/each}</select
          ></label
        >
        <label>Quiet zone (modules)<input type="number" min="4" max="16" bind:value={profile.quiet_zone} /></label>
        <label>Foreground<input type="color" bind:value={profile.foreground} /></label><label
          >Background<input type="color" bind:value={profile.background} /></label
        >
        <label class="inline"><input type="checkbox" bind:checked={profile.inverted} />Invert QR colors</label>
      </div>
    </div>
  {/if}
  {#if activeTab === 'feedback'}
    <div class="panel" role="tabpanel" id="settings-panel-feedback" aria-labelledby="settings-tab-feedback" tabindex="0">
      <div class="panel-heading">
        <h2>Feedback & ACKs</h2>
        <SectionOrder
          id={activeTab}
          title={t(tabs.find((tab) => tab.id === activeTab)!.label)}
          order={tabOrder}
          move={(direction) => layouts.move({ page: 'settings', id: activeTab, direction })}
        />
      </div>
      <div class="check-list">
        <label><input type="checkbox" bind:checked={profile.feedback} />Optical control in Both mode</label>
        <label><input type="checkbox" bind:checked={profile.ack_frames} />ACK frames and remove them from rotation</label>
        <label><input type="checkbox" bind:checked={profile.ack_files} />ACK verified files and remove their frames</label>
        <label><input type="checkbox" bind:checked={profile.stop_on_ack} />Stop the job after completion ACK</label>
        <label><input type="checkbox" bind:checked={profile.allow_unpaired_ack} />Accept unpaired acknowledgment claims</label>
      </div>
      <p class="footnote">Only receipts signed by a trusted peer can confirm delivery or trigger confirmed-delivery commands.</p>
    </div>
  {/if}
  {#if activeTab === 'files'}
    <div class="panel" role="tabpanel" id="settings-panel-files" aria-labelledby="settings-tab-files" tabindex="0">
      <div class="panel-heading">
        <h2>Files & destinations</h2>
        <SectionOrder
          id={activeTab}
          title={t(tabs.find((tab) => tab.id === activeTab)!.label)}
          order={tabOrder}
          move={(direction) => layouts.move({ page: 'settings', id: activeTab, direction })}
        />
      </div>
      <div class="form-grid">
        <label class="span-two">Receive folder<input bind:value={profile.destination} /></label>
        <label
          >Filename conflict<select bind:value={profile.conflict}
            ><option value="rename">Rename incoming file</option><option value="skip_identical">Skip if identical</option><option
              value="overwrite">Explicitly overwrite</option
            ></select
          ></label
        >
        <label>Receive quota (bytes)<input type="number" min="1" bind:value={profile.max_receive_bytes} /></label>
      </div>
      <label class="inline"><input type="checkbox" bind:checked={profile.preserve_paths} />Preserve directory structure</label>
    </div>
  {/if}
  {#if activeTab === 'camera'}
    <div class="panel" role="tabpanel" id="settings-panel-camera" aria-labelledby="settings-tab-camera" tabindex="0">
      <div class="panel-heading">
        <h2>USB camera</h2>
        <SectionOrder
          id={activeTab}
          title={t(tabs.find((tab) => tab.id === activeTab)!.label)}
          order={tabOrder}
          move={(direction) => layouts.move({ page: 'settings', id: activeTab, direction })}
        />
      </div>
      <div class="form-grid">
        <label class="span-two"
          >Device<select bind:value={profile.camera_index}
            >{#if !devices.length}<option value={0}>No webcam detected</option>{/if}{#each devices as device}<option
                value={Number(device.index)}>{device.name}</option
              >{/each}</select
          ></label
        >
        <label>Width<input type="number" bind:value={profile.camera_width} /></label><label
          >Height<input type="number" bind:value={profile.camera_height} /></label
        >
        <label>Capture FPS<input type="number" min="1" max="120" bind:value={profile.camera_fps} /></label>
        <label
          >Preview / scan rotation<select bind:value={profile.camera_rotation}
            >{#each [0, 90, 180, 270] as degrees}<option value={degrees}>{degrees}°</option>{/each}</select
          ></label
        >
        <label>Digital zoom<input type="number" min="1" max="4" step=".1" bind:value={profile.camera_zoom} /></label>
        <label class="inline"><input type="checkbox" bind:checked={profile.camera_mirror} />Mirror correction</label>
        <label class="span-two"
          >Crop [x, y, width, height] · empty for full frame<input
            value={profile.camera_crop ? JSON.stringify(profile.camera_crop) : ''}
            onchange={(event) =>
              void run(async () => {
                profile.camera_crop = event.currentTarget.value ? JSON.parse(event.currentTarget.value) : null;
              })}
          /></label
        >
      </div>
      <p class="footnote">Changes take effect when the camera restarts.</p>
      {#if status.camera?.controls?.length}
        <details>
          <summary>Live exposure, focus and device controls</summary>
          <div class="form-grid">
            <label
              >Control<select
                bind:value={controlName}
                onchange={() => {
                  controlValue = JSON.stringify(status.camera?.controls?.find((c) => c.name === controlName)?.value ?? {});
                }}
                ><option value="">Choose device control</option>{#each status.camera.controls as c}<option value={c.name}>{c.label}</option
                  >{/each}</select
              ></label
            >
            <label>Typed value<input bind:value={controlValue} placeholder="Typed control value" /></label>
          </div>
          <pre>{JSON.stringify(status.camera.controls.find((c) => c.name === controlName)?.description ?? {}, null, 2)}</pre>
          <button
            disabled={!controlName}
            onclick={() =>
              void run(async () => {
                await rpc({ method: 'camera_control', params: { name: controlName, value: JSON.parse(controlValue) } });
              })}>Set camera control</button
          >
        </details>
      {/if}
      {#if status.camera?.controlError}<p class="error">{status.camera.controlError}</p>{/if}
    </div>
  {/if}
</div>
{#if activeTab === 'peers'}
  <div class="panel" role="tabpanel" id="settings-panel-peers" aria-labelledby="settings-tab-peers" tabindex="0">
    <div class="panel-heading">
      <h2>Peer pairing</h2>
      <SectionOrder
        id={activeTab}
        title={t(tabs.find((tab) => tab.id === activeTab)!.label)}
        order={tabOrder}
        move={(direction) => layouts.move({ page: 'settings', id: activeTab, direction })}
      />
    </div>
    <p>Compare fingerprints on both devices before trusting a peer.</p>
    <div class="field-label">This device's fingerprint</div>
    <code class="digest">{status.fingerprint}</code>
    <div class="path-entry">
      <input aria-label="Trusted peer fingerprint" bind:value={peer} placeholder="SHA-256 fingerprint of the other device" /><button
        disabled={!/^[0-9a-f]{64}$/i.test(peer)}
        onclick={() => {
          if (!profile.trusted_peers.includes(peer.toLowerCase())) profile.trusted_peers = [...profile.trusted_peers, peer.toLowerCase()];
          peer = '';
        }}>Trust peer</button
      >
    </div>
    {#each profile.trusted_peers as fingerprint}<div class="rule-row">
        <code>{fingerprint}</code><button onclick={() => (profile.trusted_peers = profile.trusted_peers.filter((p) => p !== fingerprint))}
          >Remove</button
        >
      </div>{/each}
    <p class="footnote">QR transfer is visible to anyone who can record the display. File contents are not encrypted.</p>
  </div>
{/if}
{#if !['mcp', 'interface'].includes(activeTab)}
  <div class="settings-save">
    <button
      class="primary"
      onclick={() =>
        void run(async () => {
          await rpc({ method: 'set_profile', params: profile });
          await refresh();
          saved = 'Settings applied';
        })}>Apply settings</button
    >
    <button
      onclick={() =>
        void run(async () => {
          await rpc({ method: 'save_profile', params: profile });
          profiles = await rpc({ method: 'profiles' });
          saved = 'Profile saved';
        })}>Save profile</button
    >
    <select aria-label="Saved profile" bind:value={name}
      ><option value="">Select saved profile</option>{#each profiles as p}<option value={p.name}>{p.name}</option>{/each}</select
    >
    <button
      disabled={!name}
      onclick={() =>
        void run(async () => {
          await rpc({ method: 'select_profile', params: { name } });
          await refresh();
          const s = await rpc<Status>({ method: 'status' });
          profile = JSON.parse(JSON.stringify(s.profile));
        })}>Load profile</button
    >
    <span class="good" aria-live="polite">{saved}</span>
  </div>
{/if}

{#if activeTab === 'interface'}
  <div class="panel" role="tabpanel" id="settings-panel-interface" aria-labelledby="settings-tab-interface" tabindex="0">
    <div class="panel-heading">
      <h2>Interface</h2>
      <SectionOrder
        id={activeTab}
        title={t(tabs.find((tab) => tab.id === activeTab)!.label)}
        order={tabOrder}
        move={(direction) => layouts.move({ page: 'settings', id: activeTab, direction })}
      />
    </div>
    <label
      >UI scale (%)<input
        type="range"
        min="80"
        max="150"
        step="5"
        bind:value={scale}
        oninput={() => {
          document.documentElement.style.zoom = String(scale / 100);
          localStorage.setItem('qrtx-scale', String(scale));
        }}
      /></label
    >
    <p>Ctrl/Cmd + O adds files · Ctrl/Cmd + Shift + O adds a directory · Space pauses or resumes the display.</p>
  </div>
{/if}

{#if activeTab === 'mcp'}
  <div class="panel" role="tabpanel" id="settings-panel-mcp" aria-labelledby="settings-tab-mcp" tabindex="0">
    <div class="panel-heading">
      <h2>{t('qrtx-mcp_title')}</h2>
      <SectionOrder
        id={activeTab}
        title={t(tabs.find((tab) => tab.id === activeTab)!.label)}
        order={tabOrder}
        move={(direction) => layouts.move({ page: 'settings', id: activeTab, direction })}
      />
    </div>
    <div class="form-grid">
      <label class="inline"><input type="checkbox" bind:checked={mcp.enabled} />{t('qrtx-mcp_enable')}</label>
      <label>{t('qrtx-mcp_port')}<input type="number" min="1" max="65535" step="1" bind:value={mcp.port} /></label>
    </div>
    <p class="footnote">{t('qrtx-mcp_note')}</p>
    <button
      onclick={() =>
        void run(async () => {
          mcp = await rpc({ method: 'mcp_settings_save', params: mcp });
          mcpSaved = t('qrtx-mcp_saved');
        })}>{t('qrtx-mcp_save')}</button
    >
    <p class="good" aria-live="polite">{mcpSaved}</p>
  </div>
{/if}
