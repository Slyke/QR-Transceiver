<script lang="ts">
  import { onMount } from 'svelte';
  let { svg, fit = false }: { svg: string; fit?: boolean } = $props();
  let host: HTMLDivElement;
  let width = $state(300);
  let height = $state(300);
  const modules = $derived(Number(svg.match(/viewBox="0 0 (\d+)/)?.[1] ?? 1));
  const requested = $derived(Number(svg.match(/width="(\d+)"/)?.[1] ?? 640));
  const size = $derived(Math.max(1, Math.floor(Math.min(fit ? Infinity : requested, width, height) / modules)) * modules);
  onMount(() => {
    const update = () => {
      width = host.clientWidth;
      height = host.clientHeight;
    };
    const observer = new ResizeObserver(update);
    observer.observe(host);
    update();
    return () => observer.disconnect();
  });
</script>

<div class="qr-layout" bind:this={host}>
  <div style:width={size + 'px'} style:height={size + 'px'}>{@html svg}</div>
</div>

<style>
  .qr-layout {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .qr-layout :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
    image-rendering: pixelated;
  }
</style>
