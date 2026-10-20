<script lang="ts">
  // Linux build (0022): "Show Input Values" readout, for troubleshooting a controller.
  import { controllerButtonLabel, controllerConfig, controllerDebug, controllerStatus } from "./controller.svelte";

  const axis = (value: number | undefined) => (value ?? 0).toFixed(2);

  const text = $derived(
    [
      `${controllerStatus.name ?? "no controller"} · ${controllerDebug.rate} reads/s · window ${controllerDebug.focused ? "focused" : "not focused"}`,
      `left ${axis(controllerDebug.axes[0])} ${axis(controllerDebug.axes[1])} · right ${axis(controllerDebug.axes[2])} ${axis(controllerDebug.axes[3])} · buttons ${controllerDebug.buttons.map(controllerButtonLabel).join(" ") || "–"}`,
      ...(controllerDebug.scroll ? [`scroll: ${controllerDebug.scroll}`] : []),
    ].join("\n"),
  );
</script>

{#if controllerConfig.controllerShowValues}
  <div
    class="pointer-events-none fixed bottom-2 left-2 z-[9999] max-w-[calc(100vw-1rem)] rounded-md bg-black/75 px-2 py-1 font-mono text-[11px] leading-snug whitespace-pre-wrap text-white"
  >{text}</div>
{/if}
