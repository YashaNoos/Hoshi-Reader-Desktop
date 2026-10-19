export const isMac = navigator.userAgent.includes("Macintosh");
export const isWindows = navigator.userAgent.includes("Windows");
export const isChromium = navigator.userAgent.includes("Chrome/");
export const isLinux = /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent);
export const useHttpScheme = /Windows|Android/.test(navigator.userAgent);
