import { useHttpScheme } from "./platform";

// WebKitGTK cannot play <audio> from custom schemes, so on Linux the backend
// serves audiobooks over a loopback HTTP server and injects its base URL.
export function audiobookUrl(id: string): string {
  const base = (window as unknown as { __HOSHI_MEDIA__?: string }).__HOSHI_MEDIA__;
  return base ? `${base}audiobook/${id}` : schemeUrl("audiobook", id);
}

export function schemeUrl(scheme: string, path = ""): string {
  return useHttpScheme
    ? `http://${scheme}.localhost/${path}`
    : `${scheme}://localhost/${path}`;
}
