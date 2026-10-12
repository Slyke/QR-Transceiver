<script lang="ts">
  let { id, title, order, move }: { id: string; title: string; order: string[]; move: (direction: number) => void } = $props();
  const index = $derived(order.indexOf(id));
</script>

<div class="section-order-controls" role="group" aria-label={'Reorder ' + title}>
  <button aria-label={'Move ' + title + ' up'} title="Move section up" disabled={index <= 0} onclick={() => move(-1)}>↑</button>
  <button
    aria-label={'Move ' + title + ' down'}
    title="Move section down"
    disabled={index < 0 || index >= order.length - 1}
    onclick={() => move(1)}>↓</button
  >
</div>
