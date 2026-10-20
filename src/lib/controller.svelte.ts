// Linux build (0022): game controller support. Buttons trigger reader actions; the D-pad
// and left stick turn pages or move the text cursor; the right stick scrolls the popup.
// Only acts while the window has focus. On Linux the backend reads the controller
// (controller.rs: WebKitGTK's Gamepad API does not reach Hoshi's window); elsewhere the
// Gamepad API is used. Both report the W3C "standard" layout.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { persisted } from "./persisted.svelte";
import { isLinux } from "./platform";

/** Standard-layout buttons by position; what is printed on them depends on the controller. */
export const controllerButtons = [
  "Bottom",
  "Right",
  "Left",
  "Top",
  "LB",
  "RB",
  "LT",
  "RT",
  "Select",
  "Start",
  "L3",
  "R3",
  "D-pad Up",
  "D-pad Down",
  "D-pad Left",
  "D-pad Right",
  "Home",
] as const;

export type ControllerLabels = "xbox" | "nintendo" | "playstation" | "position";

export const controllerLabelStyles: { value: ControllerLabels; label: string }[] = [
  { value: "xbox", label: "Xbox (A bottom)" },
  { value: "nintendo", label: "Nintendo (B bottom)" },
  { value: "playstation", label: "PlayStation (✕ bottom)" },
  { value: "position", label: "By Position" },
];

/** Names of buttons 0–11 and 16 in each style (the D-pad keeps its names). */
const labelSets: Record<Exclude<ControllerLabels, "position">, Record<number, string>> = {
  xbox: { 0: "A", 1: "B", 2: "X", 3: "Y", 4: "LB", 5: "RB", 6: "LT", 7: "RT", 8: "View", 9: "Menu", 10: "LS", 11: "RS" },
  nintendo: { 0: "B", 1: "A", 2: "Y", 3: "X", 4: "L", 5: "R", 6: "ZL", 7: "ZR", 8: "−", 9: "+", 10: "LS", 11: "RS" },
  playstation: { 0: "✕", 1: "○", 2: "□", 3: "△", 4: "L1", 5: "R1", 6: "L2", 7: "R2", 8: "Create", 9: "Options", 10: "L3", 11: "R3" },
};

const DPAD = { up: 12, down: 13, left: 14, right: 15 } as const;

export type ControllerSection = "Reading" | "Text Cursor" | "Popup";

/** Actions in order of priority: an open popup first, then the text cursor, then reading. */
export const controllerActions = [
  { key: "playPause", label: "Play / Pause", section: "Reading", button: 0 },
  { key: "repeatCue", label: "Repeat Cue", section: "Reading", button: 2 },
  { key: "previousCue", label: "Previous Cue", section: "Reading", button: 4 },
  { key: "nextCue", label: "Next Cue", section: "Reading", button: 5 },
  { key: "toggleCursor", label: "Text Cursor On / Off", section: "Reading", button: 3 },
  { key: "toggleTracking", label: "Pause / Resume Statistics", section: "Reading", button: -1 },
  { key: "lookup", label: "Look Up", section: "Text Cursor", button: 0 },
  { key: "previousWord", label: "Previous Word", section: "Text Cursor", button: 4 },
  { key: "nextWord", label: "Next Word", section: "Text Cursor", button: 5 },
  { key: "leaveCursor", label: "Leave Text Cursor", section: "Text Cursor", button: 1 },
  { key: "mine1", label: "Mine Format 1", section: "Popup", button: 0 },
  { key: "mine2", label: "Mine Format 2", section: "Popup", button: 2 },
  { key: "mine3", label: "Mine Format 3", section: "Popup", button: -1 },
  { key: "playAudio", label: "Play Audio", section: "Popup", button: 3 },
  { key: "previousEntry", label: "Previous Entry", section: "Popup", button: 4 },
  { key: "nextEntry", label: "Next Entry", section: "Popup", button: 5 },
  { key: "closePopup", label: "Close Popup", section: "Popup", button: 1 },
] as const satisfies readonly { key: string; label: string; section: ControllerSection; button: number }[];

export type ControllerAction = (typeof controllerActions)[number]["key"];

export type ControllerConfig = {
  controllerEnabled: boolean;
  controllerLabels: ControllerLabels;
  /** In vertical text, previous/next cue and word are swapped (the left button goes forward). */
  controllerSwapVertical: boolean;
  /** Shows the controller's live values (Settings → Input → Controller), for troubleshooting. */
  controllerShowValues: boolean;
} & Record<ControllerAction, number>;

const defaults = Object.fromEntries([
  ["controllerEnabled", true],
  ["controllerLabels", "xbox"],
  ["controllerSwapVertical", false],
  ["controllerShowValues", false],
  ...controllerActions.map((action) => [action.key, action.button]),
]) as ControllerConfig;

const store = persisted<ControllerConfig>("controller.config", defaults);
export const controllerConfig = store.config;
export const saveControllerConfig = store.save;

export function resetControllerBindings() {
  for (const action of controllerActions) controllerConfig[action.key] = action.button;
  saveControllerConfig();
}

export function controllerButtonLabel(button: number): string {
  if (button < 0) return "None";
  const style = controllerConfig.controllerLabels;
  return (style !== "position" && labelSets[style]?.[button]) || (controllerButtons[button] ?? `Button ${button}`);
}

/** The button's position, for a tooltip next to its printed name. */
export function controllerButtonPosition(button: number): string {
  return button < 0 ? "" : (controllerButtons[button] ?? `Button ${button}`);
}

/** The action a button triggers, looking at the open popup, then the cursor, then reading. */
export function controllerAction(
  button: number,
  context: { popup: boolean; cursor: boolean },
): ControllerAction | null {
  const sections: ControllerSection[] = [
    ...(context.popup ? (["Popup"] as const) : []),
    ...(context.cursor ? (["Text Cursor"] as const) : []),
    "Reading",
  ];
  for (const section of sections) {
    const action = controllerActions.find(
      (entry) => entry.section === section && controllerConfig[entry.key] === button,
    );
    if (action) return action.key;
  }
  return null;
}

export type Direction = "up" | "down" | "left" | "right";
export type ControllerEvent =
  | { type: "button"; button: number }
  | { type: "direction"; direction: Direction; repeat: boolean }
  | { type: "scroll"; delta: number };

type Handler = (event: ControllerEvent) => void;

const REPEAT_DELAY = 350;
const REPEAT_INTERVAL = 90;
const STICK_DIRECTION = 0.5;
const STICK_DEADZONE = 0.2;
const TRIGGER_PRESSED = 0.5;
const SCROLL_SPEED = 24;

const handlers: Handler[] = [];
let capture: ((button: number) => void) | null = null;
let frame = 0;
let previous: boolean[] = [];
let held: { direction: Direction; since: number; last: number } | null = null;

/** Name of the controller in use, or null: updated while polling. */
export const controllerStatus = $state<{ name: string | null }>({ name: null });

/** The newest handler gets the events (Settings over the reader blocks it); returns a release. */
export function useControllerHandler(next: Handler): () => void {
  handlers.push(next);
  ensurePolling();
  return () => {
    const index = handlers.lastIndexOf(next);
    if (index >= 0) handlers.splice(index, 1);
  };
}

function handler(event: ControllerEvent) {
  handlers[handlers.length - 1]?.(event);
}

/** Resolves with the next button pressed (for Settings), or -1 when cancelled. */
export function captureControllerButton(): { promise: Promise<number>; cancel: () => void } {
  let done: (button: number) => void = () => {};
  const promise = new Promise<number>((resolve) => (done = resolve));
  capture = (button) => {
    capture = null;
    done(button);
  };
  ensurePolling();
  return {
    promise,
    cancel: () => {
      if (capture) capture(-1);
    },
  };
}

type Pad = {
  id: string;
  buttons: readonly { pressed: boolean; value: number }[];
  axes: readonly number[];
};

type NativeState = { name: string | null; buttons: number[]; axes: number[] };
let native: Pad | null = null;

function setNative(state: NativeState) {
  native = state.name
    ? {
        id: state.name,
        buttons: state.buttons.map((value) => ({ pressed: value >= 0.5, value })),
        axes: state.axes,
      }
    : null;
  controllerStatus.name = native?.id ?? null;
  if (native) ensurePolling();
}

function activePad(): Pad | null {
  if (isLinux) return native;
  const pads = (navigator.getGamepads?.() ?? []).filter((pad): pad is Gamepad => !!pad?.connected);
  return pads.find((pad) => pad.mapping === "standard") ?? pads[0] ?? null;
}

function pressed(pad: Pad, index: number): boolean {
  const button = pad.buttons[index];
  if (!button) return false;
  return index === 6 || index === 7 ? button.value >= TRIGGER_PRESSED : button.pressed;
}

function stickDirection(x: number, y: number): Direction | null {
  if (Math.max(Math.abs(x), Math.abs(y)) < STICK_DIRECTION) return null;
  if (Math.abs(x) > Math.abs(y)) return x > 0 ? "right" : "left";
  return y > 0 ? "down" : "up";
}

function direction(pad: Pad): Direction | null {
  for (const [name, index] of Object.entries(DPAD)) {
    if (pressed(pad, index)) return name as Direction;
  }
  return stickDirection(pad.axes[0] ?? 0, pad.axes[1] ?? 0);
}

/** Live values for the "Show Input Values" readout (updated about ten times a second). */
export const controllerDebug = $state({
  rate: 0,
  focused: true,
  axes: [] as number[],
  buttons: [] as number[],
  scroll: "",
});
let polls = 0;
let rateSince = 0;
let shownAt = 0;

function showValues(pad: Pad | null, now: number) {
  polls++;
  if (now - rateSince >= 1000) {
    controllerDebug.rate = Math.round((polls * 1000) / (now - rateSince));
    polls = 0;
    rateSince = now;
  }
  if (now - shownAt < 100) return;
  shownAt = now;
  controllerDebug.focused = document.hasFocus();
  controllerDebug.axes = pad ? [...pad.axes] : [];
  controllerDebug.buttons = pad ? pad.buttons.flatMap((button, index) => (button.value > 0.05 ? [index] : [])) : [];
}

function poll() {
  frame = 0;
  const pad = activePad();
  controllerStatus.name = pad?.id ?? null;
  const now = performance.now();
  if (controllerConfig.controllerShowValues) showValues(pad, now);
  if (pad && (capture || (handlers.length && controllerConfig.controllerEnabled && document.hasFocus()))) {
    const buttons = pad.buttons.map((_, index) => pressed(pad, index));
    buttons.forEach((down, index) => {
      if (!down || previous[index]) return;
      if (capture) capture(index);
      else if (!(Object.values(DPAD) as number[]).includes(index)) handler({ type: "button", button: index });
    });
    previous = buttons;

    const dir = capture ? null : direction(pad);
    if (!dir) held = null;
    else if (held?.direction !== dir) {
      held = { direction: dir, since: now, last: now };
      handler({ type: "direction", direction: dir, repeat: false });
    } else if (now - held.since >= REPEAT_DELAY && now - held.last >= REPEAT_INTERVAL) {
      held.last = now;
      handler({ type: "direction", direction: dir, repeat: true });
    }

    const y = pad.axes[3] ?? 0;
    if (!capture && Math.abs(y) > STICK_DEADZONE) {
      const strength = (Math.abs(y) - STICK_DEADZONE) / (1 - STICK_DEADZONE);
      handler({ type: "scroll", delta: Math.sign(y) * strength * strength * SCROLL_SPEED });
    }
  } else {
    previous = pad ? pad.buttons.map((_, index) => pressed(pad, index)) : [];
    held = null;
  }
  // Without a controller, wait for "gamepadconnected" instead of polling every frame.
  if (pad) ensurePolling();
}

function ensurePolling() {
  if (frame || (!handlers.length && !capture)) return;
  frame = requestAnimationFrame(poll);
}

if (isLinux) {
  // Listen first: the backend starts reading on the first "controller_state" call.
  listen<NativeState>("controller-state", (event) => setNative(event.payload))
    .then(() => invoke<NativeState>("controller_state"))
    .then(setNative, () => {});
} else {
  window.addEventListener("gamepadconnected", ensurePolling);
}
