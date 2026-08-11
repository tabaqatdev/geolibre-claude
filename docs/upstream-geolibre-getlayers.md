# Upstream proposal — expose the layer model to plugins (`getLayers` / `onLayersChange` / `getLayerSource`)

**Target repo:** [`opengeos/GeoLibre`](https://github.com/opengeos/GeoLibre)
**Status:** ready-to-submit draft (written against GeoLibre Desktop 2.5.0 source)

## Motivation

A GeoLibre plugin can *add* layers and read the live MapLibre map (`getMap()`), but it
**cannot read GeoLibre's semantic layer model** — the `name`, `type`, and especially the
**`sourcePath`** (the file a GeoParquet / GeoJSON layer was loaded from). That data lives
only in the Zustand `useAppStore`, which an external ESM plugin bundle can't import, and it
is **not** copied onto the MapLibre source (a geojson source carries only `{type, data}`).

Concretely, a plugin that mirrors the map to an external tool can *see that N layers exist*
(by diffing `map.getStyle().sources` for `source-<id>` keys) but can't recover what any
user‑added layer **is** or **where it came from** — so it can't query or round‑trip it.

The only current signal is the basemap: `getActiveBasemap()` + `onBasemapChange()`. This
proposal adds the layer‑level equivalents, reusing the **exact** store‑subscribe pattern the
host already ships for the basemap.

## The change (≈6 lines + type decls)

### 1. `apps/geolibre-desktop/src/hooks/usePlugins.ts` — inside `createAppAPI`

Add next to the existing `getActiveBasemap` / `onBasemapChange` (≈ line 997):

```ts
// Semantic layer model (read-only). Mirrors getActiveBasemap/onBasemapChange.
getLayers: () =>
  useAppStore.getState().layers.map((l) => ({
    id: l.id,
    name: l.name,
    type: l.type,
    sourcePath: l.sourcePath,
    visible: l.visible,
  })),

onLayersChange: (callback) =>
  useAppStore.subscribe((state, prev) => {
    if (state.layers !== prev.layers) callback();
  }),

// Targeted accessor for one layer's source reference.
getLayerSource: (id) => {
  const l = useAppStore.getState().layers.find((x) => x.id === id);
  return l ? { type: l.type, sourcePath: l.sourcePath, source: l.source } : null;
},
```

`useAppStore` is already in scope here (it's what every other `createAppAPI` method reads).
`onLayersChange` uses the same `useAppStore.subscribe(...)` mechanism as `onBasemapChange`.
Returning a **projection** (not the raw `GeoLibreLayer`) keeps heavy fields like inline
`geojson` out of the plugin surface.

### 2. `packages/plugins/src/types.ts` — declare on `interface GeoLibreAppAPI` (≈ line 334)

```ts
export interface GeoLibreLayerInfo {
  id: string;
  name: string;
  type: string;
  /** Absolute path for a layer read from a file on disk (GeoParquet / GeoJSON). */
  sourcePath?: string;
  visible?: boolean;
}

export interface GeoLibreAppAPI {
  // …existing…
  /** All layers currently in the project, in draw order. */
  getLayers?(): GeoLibreLayerInfo[];
  /** Subscribe to layer add/remove/reorder; returns an unsubscribe fn. */
  onLayersChange?(callback: () => void): () => void;
  /** One layer's source reference by id, or null if not found. */
  getLayerSource?(id: string): { type: string; sourcePath?: string; source: unknown } | null;
}
```

### 3. Plugin template — `opengeos/geolibre-plugin-template`, `src/lib/geolibre/host-api.ts` (≈ line 126)

Mirror the same three signatures so template consumers get typings.

## Why read-only / a projection

- No write path — plugins still mutate layers only through the existing `add*Layer` /
  `removeLayer` methods.
- The projection omits `geojson`/`style` blobs, so `getLayers()` stays cheap to call on
  every `onLayersChange`.

## Testing

- Unit: `getLayers()` returns one entry per `useAppStore.getState().layers` with matching
  `id/name/type/sourcePath`.
- Integration: load a GeoParquet file → `getLayers()[i].sourcePath` equals the file path
  shown in the layer info (“i”) panel; remove it → `onLayersChange` fires and the entry is
  gone.

## How GeoLibre-Claude uses it

The bridge already calls these **defensively** (`app.getLayers?.()`, `app.onLayersChange?.()`):
when a GeoLibre release ships them, user-added GeoParquet/GeoJSON layers immediately become
**queryable** from Claude (the MCP server resolves `sourcePath` under its allowlist). Until
then the bridge falls back to enumerating the raw MapLibre style (layer ids + geometry type
only). No coordinated release is required — it's purely additive and optional.
