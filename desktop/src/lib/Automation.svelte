<script lang="ts">
  import { onMount } from 'svelte';
  import { rpc } from './api';
  import SectionOrder from './SectionOrder.svelte';
  import { createSectionOrder } from './section-order.svelte';
  const layouts = createSectionOrder({ automation: ['api', 'mcp', 'rules', 'results'] });
  const order = $derived(layouts.get('automation'));
  let { report }: { report: (e: unknown) => void } = $props();
  let text = $state('');
  let enable = $state(false);
  let saved = $state('');
  let results = $state<{ rule: string; status: string; error: string | null }[]>([]);
  const run = async (action: () => Promise<void>) => {
    try {
      await action();
    } catch (e) {
      report(e);
    }
  };
  onMount(() => {
    void run(async () => {
      text = JSON.stringify(await rpc({ method: 'automation_config' }), null, 2);
      const data = await rpc<{ results: typeof results }>({ method: 'automation' });
      results = data.results;
    });
  });
</script>

{#snippet apiPanel()}
  <section class="panel" data-section="api">
    <div class="panel-heading">
      <h2>Local API & CLI</h2>
      <SectionOrder
        id="api"
        title="Local API & CLI"
        {order}
        move={(direction) => layouts.move({ page: 'automation', id: 'api', direction })}
      />
    </div>
    <p>Authenticated local IPC controls the persistent engine. Optional HTTP binds to loopback.</p>
    <pre>qrtransceiver status
qrtransceiver send ./outbox --recursive
qrtransceiver receive
qrtransceiver mcp</pre>
    <p>Sending is restricted to configured outgoing roots. Status, receive, send, and configuration use separate scopes.</p>
  </section>
{/snippet}
{#snippet mcpPanel()}
  <section class="panel" data-section="mcp">
    <div class="panel-heading">
      <h2>MCP tools</h2>
      <SectionOrder id="mcp" title="MCP tools" {order} move={(direction) => layouts.move({ page: 'automation', id: 'mcp', direction })} />
    </div>
    <p>Connect a local assistant over stdio with <code>qrtransceiver mcp</code>.</p>
    <p>The default MCP identity can read status. Grant send and receive scopes separately in local configuration.</p>
    <p>Mutations support a dry run. Received file contents never grant permissions.</p>
    <span class="badge live">LOCAL STDIO</span>
  </section>
{/snippet}
{#snippet rulesPanel()}
  <section class="panel" data-section="rules">
    <div class="panel-heading">
      <h2>Trusted automation rules</h2>
      <SectionOrder
        id="rules"
        title="Trusted automation rules"
        {order}
        move={(direction) => layouts.move({ page: 'automation', id: 'rules', direction })}
      />
    </div>
    <p>
      Configure outgoing roots, stable-file watches, loopback webhooks, and fixed command arguments. Received data cannot change these
      rules.
    </p>
    <details>
      <summary>Example command and watch</summary>
      <pre>{JSON.stringify(
          {
            allowed_roots: ['/home/user/outbox'],
            watches: [{ path: '/home/user/outbox', recursive: true, stable_ms: 3000 }],
            webhooks: [],
            commands: [
              {
                id: 'verified-handler',
                enabled: false,
                events: ['file.verified'],
                executable: '/usr/local/bin/process-verified-event',
                args: [],
                working_directory: '/home/user',
                timeout_ms: 10000,
                retries: 0,
                success_codes: [0],
                environment: {}
              }
            ]
          },
          null,
          2
        )}</pre>
    </details>
    <label
      >Automation configuration<textarea aria-label="Automation configuration" bind:value={text} rows="20" spellcheck="false"
      ></textarea></label
    >
    <p class="footnote">
      Commands receive event JSON on standard input. Receiver completion follows hash verification and file commit. Sender completion
      requires an authenticated receipt. Existing webhook secrets are omitted; re-enter an authentication value when editing a webhook that
      needs one.
    </p>
    <div class="panel-actions">
      <label class="inline"><input type="checkbox" bind:checked={enable} />Explicitly enable rules marked enabled</label><button
        class="primary"
        onclick={() =>
          void run(async () => {
            await rpc({ method: 'automation_save', params: { ...JSON.parse(text), enable_actions: enable } });
            saved = 'Automation configuration applied';
          })}>Apply trusted rules</button
      ><span class="good" aria-live="polite">{saved}</span>
    </div>
  </section>
{/snippet}
{#snippet resultsPanel()}
  <section class="panel" data-section="results">
    <div class="panel-heading">
      <h2>Action results</h2>
      <SectionOrder
        id="results"
        title="Action results"
        {order}
        move={(direction) => layouts.move({ page: 'automation', id: 'results', direction })}
      />
    </div>
    {#if !results.length}<p>No completion actions have run.</p>{/if}{#each results as result}<div class="rule-row">
        <strong>{result.rule}</strong><span>{result.status}</span><span class="error">{result.error ?? ''}</span>
      </div>{/each}
  </section>
{/snippet}
<div class="ordered-sections automation-sections">
  {#each order as id (id)}{#if id === 'api'}{@render apiPanel()}{:else if id === 'mcp'}{@render mcpPanel()}{:else if id === 'rules'}{@render rulesPanel()}{:else}{@render resultsPanel()}{/if}{/each}
</div>
