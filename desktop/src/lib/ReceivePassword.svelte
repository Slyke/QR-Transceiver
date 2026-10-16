<script lang="ts">
  import { rpc } from './api';
  let { id, refresh, report }: { id?: string; refresh: () => Promise<void>; report: (error: unknown) => void } = $props();
  let password = $state('');
  let busy = $state(false);
  let ready = $state(false);
  const submit = async (clear = false) => {
    busy = true;
    report('');
    try {
      await rpc({ method: 'receive_password', params: { id, password: clear ? null : password } });
      ready = !clear;
    } catch (error) {
      ready = false;
      report(error);
    } finally {
      password = '';
      busy = false;
      await refresh().catch(report);
    }
  };
</script>

<form
  class="password-entry"
  onsubmit={(event) => {
    event.preventDefault();
    void submit();
  }}
>
  <label
    >{id ? 'Password required for encrypted transfer' : 'Receive password (optional)'}
    <input
      type="password"
      autocomplete="off"
      bind:value={password}
      disabled={busy}
      aria-label={id ? 'Transfer password' : 'Receive password'}
    />
  </label>
  <div class="button-group">
    <button type="submit" disabled={busy || !password.length}>{busy ? 'Unlocking…' : id ? 'Unlock transfer' : 'Use password'}</button>
    {#if !id}<button type="button" disabled={busy} onclick={() => void submit(true)}>Clear password</button>{/if}
  </div>
  <p class="footnote">
    {id
      ? 'Scanning continues. You can retry without rescanning.'
      : ready
        ? 'Password is ready for incoming bundles until the app closes or you clear it.'
        : 'Enter before scanning or when a bundle arrives. Passwords are never saved.'}
  </p>
</form>
