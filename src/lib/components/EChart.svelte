<script lang="ts">
  import { onMount } from "svelte";
  import { echarts, type EChartsOption } from "../charts/echarts";
  import { onThemeChange } from "../charts/theme";

  interface Props {
    option: EChartsOption;
    height?: string;
  }
  let { option, height = "280px" }: Props = $props();

  let el: HTMLDivElement;
  let chart: ReturnType<typeof echarts.init> | null = null;
  // bumped on theme change so the effect re-applies the option (colors are read when option is built by the parent too)
  let themeTick = $state(0);

  function build() {
    chart?.dispose();
    chart = echarts.init(el, undefined, { renderer: "canvas" });
    themeTick++;
  }

  onMount(() => {
    build();
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    const off = onThemeChange(build);
    return () => {
      off();
      ro.disconnect();
      chart?.dispose();
      chart = null;
    };
  });

  $effect(() => {
    themeTick;
    chart?.setOption({ backgroundColor: "transparent", ...option }, { notMerge: true });
  });
</script>

<div bind:this={el} class="chart" style:height></div>

<style>
  .chart {
    width: 100%;
  }
</style>
