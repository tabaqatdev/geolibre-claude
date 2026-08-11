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

/** GeoLibre's semantic layer model — id, friendly name, type, and the file
 *  reference. Exposed only via a (proposed) `getLayers()`; see docs/upstream-*. */
interface GeoLibreLayerInfo {
  id: string;
  name?: string;
  type?: string;
  sourcePath?: string;
}

interface GeoLibreAppAPI {
  getMap(): MapLike | null;
  fitBounds(bounds: [number, number, number, number]): void;
  addGeoJsonLayer(name: string, data: unknown, sourcePath?: string): string;
  setBasemap?(styleUrl: string): void;
  // Live reads (present at runtime in GeoLibre ≥2.5; call defensively).
  getActiveBasemap?(): string;
  // Semantic layer model — present once the upstream getLayers() lands; until then
  // the bridge falls back to enumerating the raw MapLibre style. See docs/upstream-*.
  getLayers?(): GeoLibreLayerInfo[];
  onLayersChange?(cb: () => void): () => void;
  getLayerSource?(id: string): { type?: string; sourcePath?: string; source?: unknown } | null;
  registerToolbarMenu?(menu: unknown): (() => void) | void;
  registerRightPanel?(panel: unknown): (() => void) | void;
  openRightPanel?(id: string): void;
  fetchArrayBuffer?(url: string): Promise<ArrayBuffer>;
}

interface MapStyleLayer {
  id: string;
  type: string;
  source?: string;
}
interface MapStyle {
  layers?: MapStyleLayer[];
  sources?: Record<string, unknown>;
}
interface MapLike {
  setPaintProperty?(layer: string, prop: string, value: unknown): void;
  setCenter?(c: [number, number]): void;
  setZoom?(z: number): void;
  getStyle?(): MapStyle | undefined;
  on?(type: string, listener: (...args: unknown[]) => void): void;
  off?(type: string, listener: (...args: unknown[]) => void): void;
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

// ── Bridge configuration ──────────────────────────────────────────────────────
// GeoLibre Desktop only exposes a FIXED allowlist of AI-provider keys through
// `window.__GEOLIBRE_RUNTIME_ENV__` ("GeoLibre never reads any other system
// variable"), so custom GEOLIBRE_CLAUDE_* env vars can never reach a plugin. Config
// therefore resolves in priority order: the in-panel form (persisted in the webview's
// localStorage) → the runtime-env allowlist (if a fork ever wires these keys) →
// localhost defaults. The endpoints live on the server's HTTPS listener; GeoLibre's
// CSP permits `https:` and `http://localhost:*` in connect-src (never `file:`).
const CONFIG_LS_KEY = "geolibre-claude-bridge.config";
const DEFAULT_BASE = "https://localhost:8443";
const PROJECT_URL_KEY = "GEOLIBRE_CLAUDE_PROJECT_URL";
const CONTEXT_URL_KEY = "GEOLIBRE_CLAUDE_CONTEXT_URL";
const AUTH_TOKEN_KEY = "GEOLIBRE_CLAUDE_AUTH_TOKEN";
const POLL_MS = 2000;

interface BridgeConfig {
  projectUrl: string;
  contextUrl: string;
  token: string;
}

/** Resolve config: saved (localStorage) → runtime-env allowlist → localhost defaults. */
function loadConfig(): BridgeConfig {
  let saved: Partial<BridgeConfig> = {};
  try {
    saved = JSON.parse(globalThis.localStorage?.getItem(CONFIG_LS_KEY) || "{}");
  } catch {
    /* ignore malformed storage */
  }
  const env = runtimeEnv();
  return {
    projectUrl: saved.projectUrl || env[PROJECT_URL_KEY] || `${DEFAULT_BASE}/project`,
    contextUrl: saved.contextUrl || env[CONTEXT_URL_KEY] || `${DEFAULT_BASE}/context`,
    token: saved.token ?? env[AUTH_TOKEN_KEY] ?? "",
  };
}

/** Persist a config patch to localStorage (the token stays machine-local, never in a project file). */
function saveConfig(patch: Partial<BridgeConfig>): BridgeConfig {
  const next = { ...loadConfig(), ...patch };
  try {
    globalThis.localStorage?.setItem(CONFIG_LS_KEY, JSON.stringify(next));
  } catch {
    /* ignore storage failures */
  }
  return next;
}

/** Authorization header for the server endpoints, when a bearer token is configured. */
function authHeaders(): Record<string, string> {
  const token = loadConfig().token;
  return token ? { Authorization: `Bearer ${token}` } : {};
}

const applied = new Map<string, string>(); // project layer id → map layer id
let timer: ReturnType<typeof setInterval> | undefined;
let unsubscribeLayers: (() => void) | undefined; // live layer add/remove listener
let lastSerialized = "";
let lastProject: Project = { version: "0" }; // last applied project, for context refreshes

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
        for (const [prop, value] of paintFor(layer.style)) map?.setPaintProperty?.(mapLayerId, prop, value);
      }
    } catch (e) {
      console.error(`[geolibre-claude] failed to apply layer ${layer.id}:`, e);
    }
  }

  const view = project.view;
  if (view?.bounds) app.fitBounds(view.bounds);
  else if (view?.center) {
    const map = app.getMap();
    map?.setCenter?.(view.center);
    if (view.zoom != null) map?.setZoom?.(view.zoom);
  }

  // Report the resulting map back to the server so describe_layer / query_data can
  // see the layers (and resolve tokens). A production plugin also includes layers the
  // user added directly in GeoLibre, read from the app's layer manager.
  await sendMapContext(app, project);
}

/** Map a project-document source to the map-context source the server expects. */
function toContextSource(src: ProjectLayer["source"]): Record<string, unknown> {
  if (src.type === "arcgis") return { type: "arcgis", url: src.layer }; // add `token` from app auth if secured
  if (src.type === "geoparquet") return { type: "geoparquet", path: src.path };
  return { type: "geojson" };
}

/** Map a MapLibre style-layer type to a coarse geometry kind. */
function geometryForStyleType(t: string): string | undefined {
  switch (t) {
    case "circle":
    case "heatmap":
    case "symbol":
      return "point";
    case "line":
      return "line";
    case "fill":
    case "fill-extrusion":
      return "polygon";
    case "raster":
      return "raster";
    default:
      return undefined;
  }
}

/** Read the LIVE layer set from the MapLibre map. GeoLibre names each layer's source
 *  `source-<layerId>` (packages/map geojson-loader), so we recover every rendered
 *  layer — including ones the user added directly in GeoLibre — plus a coarse geometry
 *  type. Friendly name / sourcePath aren't exposed to plugins yet (that needs an
 *  upstream `getLayers()`), so user-added layers are reported id-only. */
function liveLayers(app: GeoLibreAppAPI): Array<{ id: string; geometryType?: string }> {
  const style = app.getMap?.()?.getStyle?.();
  if (!style) return [];
  const geomById: Record<string, string | undefined> = {};
  for (const sl of style.layers ?? []) {
    const src = sl.source;
    if (typeof src === "string" && src.startsWith("source-")) {
      const id = src.slice("source-".length);
      if (!geomById[id]) geomById[id] = geometryForStyleType(sl.type);
    }
  }
  return Object.keys(geomById).map((id) => ({ id, geometryType: geomById[id] }));
}

/** Report the live map to the server (plugin → server) via `PUT /context`. Combines
 *  the layers this bridge applied (full, queryable metadata) with everything actually
 *  rendered on the map (via getMap()/getStyle) so Claude sees user-added layers too,
 *  plus the active basemap. The target is an https/localhost URL the webview CSP
 *  permits (`file://` is blocked by CSP and unsupported by fetch). */
/** Turn a layer's file reference into a queryable map-context source. GeoParquet and
 *  referenced GeoJSON become server-queryable (the server resolves the path under its
 *  allowlist); a layer with no path is app-only (visible, not queryable). */
function classifySource(sourcePath?: string): Record<string, unknown> {
  if (!sourcePath) return { type: "app" };
  const lower = sourcePath.toLowerCase();
  if (lower.endsWith(".parquet") || lower.endsWith(".geoparquet")) return { type: "geoparquet", path: sourcePath };
  if (lower.endsWith(".geojson") || lower.endsWith(".json")) return { type: "geojson", path: sourcePath };
  return { type: "file", path: sourcePath };
}

async function sendMapContext(app: GeoLibreAppAPI, project: Project): Promise<void> {
  const target = loadConfig().contextUrl;
  if (!target) return; // no write target configured — stay idle rather than guess

  // Layers this bridge applied → richest metadata (name + real source), by map id.
  const known = new Map<string, { id: string; name: string; source: Record<string, unknown> }>();
  for (const l of project.layers ?? []) {
    known.set(applied.get(l.id) ?? l.id, { id: l.id, name: l.name, source: toContextSource(l.source) });
  }

  let layers: Array<Record<string, unknown>>;
  const semantic = app.getLayers?.();
  if (Array.isArray(semantic)) {
    // Best case: GeoLibre exposes the real layer model — every layer (incl.
    // user-added) carries its name + sourcePath, so GeoParquet/GeoJSON are queryable.
    layers = semantic.map((l) => {
      const k = known.get(l.id);
      return {
        id: l.id,
        name: l.name ?? k?.name ?? l.id,
        // Prefer the file reference; fall back to what the bridge itself added.
        source: l.sourcePath ? classifySource(l.sourcePath) : k?.source ?? { type: "app" },
        layerType: l.type,
      };
    });
  } else {
    // Fallback (GeoLibre without getLayers()): enumerate the raw MapLibre style —
    // ids + geometry only. User-added layers are visible but not queryable.
    layers = [];
    const seen = new Set<string>();
    for (const live of liveLayers(app)) {
      seen.add(live.id);
      const k = known.get(live.id);
      layers.push(
        k
          ? { ...k, geometryType: live.geometryType }
          : { id: live.id, name: live.id, source: { type: "app" }, geometryType: live.geometryType, addedInApp: true },
      );
    }
    for (const [mapId, k] of known) if (!seen.has(mapId)) layers.push(k);
  }

  const body = { layers, basemap: app.getActiveBasemap?.() || undefined, view: project.view };
  try {
    await fetch(target, {
      method: "PUT",
      headers: { "Content-Type": "application/json", ...authHeaders() },
      body: JSON.stringify(body),
    });
  } catch (e) {
    console.error("[geolibre-claude] sendMapContext failed:", e);
  }
}

async function loadOnce(app: GeoLibreAppAPI): Promise<void> {
  const url = loadConfig().projectUrl; // always set (localhost default); token gates access
  try {
    const res = await fetch(url, { headers: authHeaders() });
    if (res.ok) {
      const text = await res.text();
      if (text !== lastSerialized) {
        lastSerialized = text;
        lastProject = JSON.parse(text) as Project;
        await applyProject(app, lastProject); // applyProject reports context itself
        return;
      }
    }
    // Project unchanged (or 404/401): still push the live map state each poll so
    // layers the user added directly in GeoLibre are reflected without a Claude edit.
    await sendMapContext(app, lastProject);
  } catch (e) {
    console.error("[geolibre-claude] loadOnce failed:", e);
  }
}

/** The bridge's config panel: base URL (defaults to localhost:8443) + bearer token.
 *  This is how the plugin receives its credentials — GeoLibre Desktop can't pass
 *  custom env vars to plugins, so the token is entered here and kept in localStorage. */
function renderConfigPanel(app: GeoLibreAppAPI, container: HTMLElement): () => void {
  const cfg = loadConfig();
  const baseFromUrl = cfg.projectUrl.replace(/\/project\/?$/, "");
  container.innerHTML = "";

  const wrap = document.createElement("div");
  wrap.style.cssText = "padding:12px;font:13px/1.5 system-ui,-apple-system,sans-serif;display:flex;flex-direction:column;gap:10px";

  const status = document.createElement("div");
  status.style.cssText = "padding:8px 10px;border-radius:6px;background:rgba(127,127,127,.12)";
  const setStatus = (msg: string) => (status.textContent = msg);

  const field = (labelText: string, input: HTMLInputElement) => {
    const label = document.createElement("label");
    label.style.cssText = "display:flex;flex-direction:column;gap:4px;font-weight:600";
    label.append(labelText);
    input.style.cssText = "font:inherit;padding:6px 8px;border:1px solid rgba(127,127,127,.4);border-radius:6px;font-weight:400";
    label.append(input);
    return label;
  };

  const urlInput = document.createElement("input");
  urlInput.type = "text";
  urlInput.value = baseFromUrl;
  urlInput.placeholder = DEFAULT_BASE;

  const tokenInput = document.createElement("input");
  tokenInput.type = "password";
  tokenInput.value = cfg.token;
  tokenInput.placeholder = "paste your server token";

  const btn = document.createElement("button");
  btn.textContent = "Save & connect";
  btn.style.cssText = "font:inherit;font-weight:600;padding:7px 10px;border:none;border-radius:6px;background:#0ea5e9;color:#fff;cursor:pointer";
  btn.onclick = () => {
    const base = (urlInput.value.trim() || DEFAULT_BASE).replace(/\/$/, "");
    saveConfig({ token: tokenInput.value.trim(), projectUrl: `${base}/project`, contextUrl: `${base}/context` });
    setStatus("Connecting…");
    void loadOnce(app).then(() => {
      const c = loadConfig();
      setStatus(c.token ? `Watching ${c.projectUrl}` : "Enter a token to connect.");
    });
  };

  setStatus(cfg.token ? `Watching ${cfg.projectUrl}` : "Enter your server token to connect.");
  wrap.append(status, field("Server URL", urlInput), field("Server token", tokenInput), btn);
  container.append(wrap);
  return () => (container.innerHTML = "");
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
      render: (container: HTMLElement) => renderConfigPanel(app, container),
    });

    // Poll the project document, and push map state back to the server.
    void loadOnce(app);
    timer = setInterval(() => void loadOnce(app), POLL_MS);

    // React to layer add/remove immediately (not just on the poll). Prefer the
    // semantic hook once GeoLibre ships it; otherwise the MapLibre `styledata`
    // event, debounced 250ms like the host's own layer-control refresh.
    if (app.onLayersChange) {
      unsubscribeLayers = app.onLayersChange(() => void loadOnce(app));
    } else {
      const map = app.getMap?.();
      if (map?.on) {
        let debounce: ReturnType<typeof setTimeout> | undefined;
        const handler = () => {
          if (debounce) clearTimeout(debounce);
          debounce = setTimeout(() => void loadOnce(app), 250);
        };
        map.on("styledata", handler);
        unsubscribeLayers = () => map.off?.("styledata", handler);
      }
    }
    return true;
  },

  deactivate() {
    if (timer) clearInterval(timer);
    timer = undefined;
    unsubscribeLayers?.();
    unsubscribeLayers = undefined;
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
