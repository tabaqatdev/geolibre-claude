/**
 * GeoLibre-Claude Bridge — a standard GeoLibre plugin (TS/ESM).
 *
 * It applies the project document that the `geolibre-claude` MCP server maintains
 * (`claude.geolibre.json`) to the live map, so Claude can add/style/zoom layers.
 *
 * Two halves:
 *  - applyProject(): turns a project document into GeoLibreAppAPI calls. Real and
 *    reviewable; the ArcGIS catalog fetch is a normal HTTPS GET.
 *  - loadLoop(): reads the project document on an interval from the server's
 *    `GET /project` endpoint (and reports the map back via `PUT /context`). Both are
 *    http(s)/localhost URLs, which GeoLibre's webview CSP permits (connect-src
 *    `https:` / `http://localhost:*`; `file:` is blocked). The URL comes from the
 *    allowlisted runtime env; without it the bridge stays idle rather than guessing.
 */

interface GeoLibreAppAPI {
  getMap(): MapLike;
  fitBounds(bounds: [number, number, number, number]): void;
  addGeoJsonLayer(name: string, data: unknown, sourcePath?: string): string;
  setBasemap?(styleUrl: string): void;
  registerToolbarMenu?(menu: unknown): (() => void) | void;
  registerRightPanel?(panel: unknown): (() => void) | void;
  openRightPanel?(id: string): void;
  fetchArrayBuffer?(url: string): Promise<ArrayBuffer>;
}

interface MapLike {
  setPaintProperty?(layer: string, prop: string, value: unknown): void;
  setCenter?(c: [number, number]): void;
  setZoom?(z: number): void;
}

interface GeoLibrePlugin {
  id: string;
  name: string;
  version: string;
  activate: (app: GeoLibreAppAPI) => boolean | void;
  deactivate: (app: GeoLibreAppAPI) => void;
  getProjectState?: () => unknown;
  applyProjectState?: (app: GeoLibreAppAPI, state: unknown) => boolean | void;
}

// ── project document shapes (mirror the Rust `app::Project`) ──────────────────
interface ProjectLayer {
  id: string;
  name: string;
  source:
    | { type: "geojson"; data: unknown }
    | { type: "arcgis"; layer: string; where?: string } // layer = full Feature Service layer URL
    | { type: "geoparquet"; path: string };
  style?: SymbologySpec;
  visible?: boolean;
}
interface SymbologySpec {
  type: "simple" | "graduated" | "categorized";
  field?: string;
  color?: string;
  opacity?: number;
  classes?: Array<{ min?: number; max?: number | null; value?: string; color: string }>;
}
interface Project {
  version: string;
  view?: { bounds?: [number, number, number, number]; center?: [number, number]; zoom?: number };
  basemap?: string;
  layers?: ProjectLayer[];
}

// The server exposes these over its HTTPS listener as GET /project and PUT /context.
// GeoLibre's webview CSP allows `https:` and `http://localhost:*` in connect-src (but
// NOT `file:`), so these MUST be http(s) URLs — e.g. https://localhost:8443/project.
const PROJECT_URL_KEY = "GEOLIBRE_CLAUDE_PROJECT_URL"; // GET claude.geolibre.json (server → plugin)
const CONTEXT_URL_KEY = "GEOLIBRE_CLAUDE_CONTEXT_URL"; // PUT map-context.json (plugin → server)
const AUTH_TOKEN_KEY = "GEOLIBRE_CLAUDE_AUTH_TOKEN"; // bearer token the endpoints require (if set)
const POLL_MS = 2000;

/** Authorization header for the server endpoints, when a bearer token is configured. */
function authHeaders(): Record<string, string> {
  const token = runtimeEnv()[AUTH_TOKEN_KEY];
  return token ? { Authorization: `Bearer ${token}` } : {};
}

const applied = new Map<string, string>(); // project layer id → map layer id
let timer: ReturnType<typeof setInterval> | undefined;
let lastSerialized = "";

function runtimeEnv(): Record<string, string> {
  return (globalThis as { __GEOLIBRE_RUNTIME_ENV__?: Record<string, string> }).__GEOLIBRE_RUNTIME_ENV__ ?? {};
}

async function resolveLayerData(app: GeoLibreAppAPI, layer: ProjectLayer): Promise<unknown | undefined> {
  const src = layer.source;
  if (src.type === "geojson") return src.data;
  if (src.type === "arcgis") {
    // `layer` is the full Feature Service layer URL; fetch it as GeoJSON.
    const q = new URLSearchParams({ where: src.where || "1=1", outFields: "*", f: "geojson", returnGeometry: "true" });
    const res = await fetch(`${src.layer.replace(/\/$/, "")}/query?${q.toString()}`);
    if (!res.ok) throw new Error(`arcgis fetch ${res.status} for ${layer.id}`);
    return res.json();
  }
  // geoparquet: read client-side via the host's DuckDB-WASM. This is the one path
  // not yet wired end-to-end (needs the app's DuckDB handle); skip with a note.
  console.warn(`[geolibre-claude] geoparquet layer ${layer.id} needs client-side DuckDB (pending)`);
  return undefined;
}

/** Turn a symbology spec into a minimal MapLibre fill paint. Best-effort. */
function paintFor(style: SymbologySpec): Array<[string, unknown]> {
  const opacity = style.opacity ?? 0.85;
  if (style.type === "simple" && style.color) {
    return [["fill-color", style.color], ["fill-opacity", opacity]];
  }
  if (style.type === "graduated" && style.field && style.classes?.length) {
    const expr: unknown[] = ["step", ["get", style.field], style.classes[0].color];
    for (const c of style.classes.slice(1)) if (c.min != null) expr.push(c.min, c.color);
    return [["fill-color", expr], ["fill-opacity", opacity]];
  }
  if (style.type === "categorized" && style.field && style.classes?.length) {
    const expr: unknown[] = ["match", ["get", style.field]];
    for (const c of style.classes) if (c.value != null) expr.push(c.value, c.color);
    expr.push("#cccccc");
    return [["fill-color", expr], ["fill-opacity", opacity]];
  }
  return [["fill-opacity", opacity]];
}

async function applyProject(app: GeoLibreAppAPI, project: Project): Promise<void> {
  if (project.basemap) app.setBasemap?.(project.basemap);

  const wanted = new Set((project.layers ?? []).map((l) => l.id));
  // Layers no longer in the document: best-effort forget (host removes on reload).
  for (const id of [...applied.keys()]) if (!wanted.has(id)) applied.delete(id);

  for (const layer of project.layers ?? []) {
    if (layer.visible === false) continue;
    try {
      const data = await resolveLayerData(app, layer);
      if (data === undefined) continue;
      const mapLayerId = app.addGeoJsonLayer(layer.name, data);
      applied.set(layer.id, mapLayerId);
      if (layer.style) {
        const map = app.getMap();
        for (const [prop, value] of paintFor(layer.style)) map.setPaintProperty?.(mapLayerId, prop, value);
      }
    } catch (e) {
      console.error(`[geolibre-claude] failed to apply layer ${layer.id}:`, e);
    }
  }

  const view = project.view;
  if (view?.bounds) app.fitBounds(view.bounds);
  else if (view?.center) {
    const map = app.getMap();
    map.setCenter?.(view.center);
    if (view.zoom != null) map.setZoom?.(view.zoom);
  }

  // Report the resulting map back to the server so describe_layer / query_data can
  // see the layers (and resolve tokens). A production plugin also includes layers the
  // user added directly in GeoLibre, read from the app's layer manager.
  await sendMapContext(project);
}

/** Map a project-document source to the map-context source the server expects. */
function toContextSource(src: ProjectLayer["source"]): Record<string, unknown> {
  if (src.type === "arcgis") return { type: "arcgis", url: src.layer }; // add `token` from app auth if secured
  if (src.type === "geoparquet") return { type: "geoparquet", path: src.path };
  return { type: "geojson" };
}

/** Report the live map to the server (plugin → server) via `PUT /context`. The
 *  target is an https/localhost URL the webview CSP permits (connect-src `https:` /
 *  `http://localhost:*`); `file://` is blocked by CSP and unsupported by fetch. */
async function sendMapContext(project: Project): Promise<void> {
  const target = runtimeEnv()[CONTEXT_URL_KEY];
  if (!target) return; // no write target configured — stay idle rather than guess
  const layers = (project.layers ?? []).map((l) => ({ id: l.id, name: l.name, source: toContextSource(l.source) }));
  try {
    await fetch(target, {
      method: "PUT",
      headers: { "Content-Type": "application/json", ...authHeaders() },
      body: JSON.stringify({ layers, view: project.view }),
    });
  } catch (e) {
    console.error("[geolibre-claude] sendMapContext failed:", e);
  }
}

async function loadOnce(app: GeoLibreAppAPI): Promise<void> {
  const url = runtimeEnv()[PROJECT_URL_KEY];
  if (!url) return; // no source configured — stay idle rather than guess (see file header)
  try {
    const res = await fetch(url, { headers: authHeaders() });
    if (!res.ok) return; // 404 = no project document yet; nothing to apply
    const text = await res.text();
    if (text === lastSerialized) return; // unchanged
    lastSerialized = text;
    await applyProject(app, JSON.parse(text) as Project);
  } catch (e) {
    console.error("[geolibre-claude] loadOnce failed:", e);
  }
}

const plugin: GeoLibrePlugin = {
  id: "geolibre-claude-bridge",
  name: "GeoLibre-Claude Bridge",
  version: "0.0.1",

  activate(app) {
    app.registerToolbarMenu?.({
      id: "geolibre-claude-menu",
      label: "Claude",
      items: [{ id: "open", label: "Bridge panel", onSelect: () => app.openRightPanel?.("geolibre-claude-bridge") }],
    });
    app.registerRightPanel?.({
      id: "geolibre-claude-bridge",
      title: "GeoLibre-Claude",
      defaultWidth: 340,
      render(container: HTMLElement) {
        const src = runtimeEnv()[PROJECT_URL_KEY];
        container.textContent = src
          ? `Bridge active — watching ${src}`
          : `Bridge active. Set ${PROJECT_URL_KEY} in the runtime env to apply Claude's map document.`;
        return () => (container.textContent = "");
      },
    });

    // Poll the project document. Verify this path in a running GeoLibre (CSP).
    void loadOnce(app);
    timer = setInterval(() => void loadOnce(app), POLL_MS);
    return true;
  },

  deactivate() {
    if (timer) clearInterval(timer);
    timer = undefined;
    applied.clear();
  },

  // Persist bridge settings into `.geolibre.json` (the project-file channel).
  getProjectState() {
    return { version: plugin.version };
  },
  applyProjectState(app, state) {
    if (state && typeof state === "object") void applyProject(app, state as Project);
    return true;
  },
};

export default plugin;
export { plugin };
