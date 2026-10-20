<script lang="ts">
  // Linux build (0022): controller settings, at the end of Settings → Input.
  import SettingsSection from "./SettingsSection.svelte";
  import SettingRow from "./SettingRow.svelte";
  import SettingToggle from "./SettingToggle.svelte";
  import Toast from "./Toast.svelte";
  import { isLinux } from "./platform";
  import {
    captureControllerButton,
    controllerActions,
    controllerButtonLabel,
    controllerButtonPosition,
    controllerConfig,
    controllerLabelStyles,
    controllerStatus,
    resetControllerBindings,
    saveControllerConfig,
    useControllerHandler,
    type ControllerAction,
    type ControllerSection,
  } from "./controller.svelte";

  const sections: ControllerSection[] = ["Reading", "Text Cursor", "Popup"];

  let binding = $state<ControllerAction | null>(null);
  let toast = $state<Toast>();
  let cancelCapture: (() => void) | null = null;

  // Polls while Settings is open, so the connected controller's name shows; buttons
  // pressed here do nothing in a reader underneath.
  $effect(() => {
    const release = useControllerHandler(() => {});
    return () => {
      cancelCapture?.();
      release();
    };
  });

  async function bind(key: ControllerAction) {
    cancelCapture?.();
    binding = key;
    const capture = captureControllerButton();
    cancelCapture = capture.cancel;
    const button = await capture.promise;
    if (binding !== key) return;
    binding = null;
    cancelCapture = null;
    if (button < 0) return;
    const action = controllerActions.find((entry) => entry.key === key)!;
    const conflict = controllerActions.find(
      (entry) => entry.key !== key && entry.section === action.section && controllerConfig[entry.key] === button,
    );
    if (conflict) {
      toast?.show(
        `${controllerButtonLabel(button)} is already used by ${conflict.label} (${conflict.section}).\nClear it there first, or choose another button.`,
        "error",
      );
      return;
    }
    if (button >= 12 && button <= 15) {
      toast?.show("The D-pad turns pages and moves the text cursor: it cannot be assigned.", "error");
      return;
    }
    controllerConfig[key] = button;
    saveControllerConfig();
  }

  function clear(key: ControllerAction) {
    cancelCapture?.();
    binding = null;
    controllerConfig[key] = -1;
    saveControllerConfig();
  }
</script>

<SettingsSection title="Controller">
  <SettingToggle label="Use Controller" bind:checked={controllerConfig.controllerEnabled} onchange={saveControllerConfig} />
  <p class="text-xs text-base-content/60">
    {controllerStatus.name
      ? `Connected: ${controllerStatus.name}`
      : isLinux
        ? "No controller connected."
        : "No controller seen yet: press a button on it."}
  </p>
  <p class="text-xs text-base-content/60">
    D-pad or left stick: turn pages, or move the text cursor. Right stick: scroll the popup. Buttons act on the open
    popup first, then on the text cursor, then on reading.
  </p>
  <SettingRow label="Button Names">
    <select class="select select-sm w-48" bind:value={controllerConfig.controllerLabels} onchange={saveControllerConfig}>
      {#each controllerLabelStyles as style (style.value)}
        <option value={style.value}>{style.label}</option>
      {/each}
    </select>
  </SettingRow>
  <p class="text-xs text-base-content/60">
    Choose the letters printed on your controller. The buttons work by position, so with "Nintendo" the bottom button
    is B.
  </p>
  <SettingToggle
    label="Swap Previous / Next in Vertical Text"
    bind:checked={controllerConfig.controllerSwapVertical}
    onchange={saveControllerConfig}
  />
  <p class="text-xs text-base-content/60">
    For cues and words: in vertical text the left button goes forward, like the pages.
  </p>
  <SettingToggle
    label="Show Input Values"
    bind:checked={controllerConfig.controllerShowValues}
    onchange={saveControllerConfig}
  />
  <p class="text-xs text-base-content/60">
    For troubleshooting: shows the stick values, how often the controller is read, and where the right stick scrolls
    the popup, in a corner of the window.
  </p>
</SettingsSection>

{#each sections as section (section)}
  <SettingsSection title="Controller: {section}">
    {#each controllerActions.filter((action) => action.section === section) as action (action.key)}
      <SettingRow label={action.label}>
        <div class="join">
          <button
            class="btn join-item btn-sm min-w-28 {binding === action.key ? 'btn-active' : ''}"
            title={controllerButtonPosition(controllerConfig[action.key])}
            onclick={() => bind(action.key)}
          >
            {binding === action.key ? "Press a button…" : controllerButtonLabel(controllerConfig[action.key])}
          </button>
          <button
            class="btn join-item btn-sm"
            aria-label="Clear"
            title="Clear"
            disabled={controllerConfig[action.key] < 0}
            onclick={() => clear(action.key)}
          >
            ×
          </button>
        </div>
      </SettingRow>
    {/each}
  </SettingsSection>
{/each}

<Toast bind:this={toast} />
<div class="flex justify-end">
  <button class="btn btn-ghost btn-sm" onclick={resetControllerBindings}>Reset Controller Buttons</button>
</div>
