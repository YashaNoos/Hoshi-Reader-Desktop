import { invoke } from "@tauri-apps/api/core";
import type { Menu } from "@tauri-apps/api/menu";
import { isLinux } from "./platform";

// On Linux (Wayland) a position-less popup has no parent window and lands in the
// wrong place, so anchor it at the pointer inside our window instead.
export function showContextMenu(menu: Menu): Promise<void> {
  return isLinux ? invoke("popup_menu_at_pointer", { rid: menu.rid }) : menu.popup();
}
