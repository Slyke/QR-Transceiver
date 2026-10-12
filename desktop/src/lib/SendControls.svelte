<script lang="ts">
  import { t } from './i18n';
  import type { Profile, DisplaySettings } from './types';
  let { profile, change }: { profile?: Profile; change: (settings: DisplaySettings) => void } = $props();
  const numeric = (event: Event, key: 'frame_ms' | 'qr_pixels' | 'quiet_zone') => {
    const input = event.currentTarget as HTMLInputElement;
    if (!input.value || !input.validity.valid) return;
    change({ [key]: key === 'frame_ms' ? Math.round(1000 / input.valueAsNumber) : input.valueAsNumber });
  };
</script>

{#if profile}
  <details class="send-controls">
    <summary>{t('qrtx-send_controls')} <span>{(1000 / profile.frame_ms).toFixed(1)} FPS</span></summary>
    <div class="form-grid">
      <label
        >{t('qrtx-send_fps')}<input
          type="number"
          min="0.02"
          max="20"
          step="any"
          value={Number((1000 / profile.frame_ms).toFixed(2))}
          oninput={(event) => numeric(event, 'frame_ms')}
        /></label
      >
      <label
        >{t('qrtx-qr_size')}<input
          type="number"
          min="128"
          max="4096"
          step="1"
          value={profile.qr_pixels}
          oninput={(event) => numeric(event, 'qr_pixels')}
        /></label
      >
      <label
        >{t('qrtx-quiet_zone')}<input
          type="number"
          min="4"
          max="16"
          step="1"
          value={profile.quiet_zone}
          oninput={(event) => numeric(event, 'quiet_zone')}
        /></label
      >
      <label class="inline"
        ><input type="checkbox" checked={profile.inverted} onchange={(event) => change({ inverted: event.currentTarget.checked })} />{t(
          'qrtx-invert'
        )}</label
      >
    </div>
    <p class="footnote">{t('qrtx-send_live')}</p>
  </details>
{/if}
