import type { TubeMapLayout } from "./tube-map-layout.js";

export interface TubeMapRenderOptions {
  readonly selectedNodeKey?: string;
  readonly selectedPatternId?: string;
  readonly showTileBoundaries?: boolean;
  readonly onNodeSelect?: (nodeKey: string) => void;
  readonly onPatternSelect?: (patternId: string) => void;
}

export interface TubeMapRenderResult {
  readonly svgElements: number;
  destroy(): void;
}

const SVG_NS = "http://www.w3.org/2000/svg";
const PATTERN_COLORS = [
  "#a855f7",
  "#0891b2",
  "#db2777",
  "#ca8a04",
  "#059669",
  "#4f46e5",
];

/** Draw a layout into an existing SVG without a framework dependency. */
export function renderTubeMapSvg(
  svg: SVGSVGElement,
  layout: TubeMapLayout,
  options: TubeMapRenderOptions = {},
): TubeMapRenderResult {
  svg.replaceChildren();
  svg.setAttribute("viewBox", `0 0 ${layout.width} ${layout.height}`);
  svg.setAttribute("role", "img");
  svg.setAttribute(
    "aria-label",
    `Tube map for ${layout.model.query.sample} ${layout.model.query.contig}:${layout.model.query.start}-${layout.model.query.end}`,
  );
  svg.classList.add("pngr-tube-map-svg");
  const listeners: Array<() => void> = [];
  const root = svgElement("g", { class: "pngr-tube-map" });
  svg.append(root);

  if (options.showTileBoundaries !== false) {
    const group = svgElement("g", {
      class: "pngr-tile-boundaries",
      "aria-hidden": "true",
    });
    for (const boundary of layout.tileBoundaries) {
      group.append(
        svgElement("line", {
          x1: boundary.x,
          x2: boundary.x,
          y1: 34,
          y2: layout.height - 28,
          "data-tile-key": boundary.tileKey,
        }),
        svgText(boundary.x + 5, 48, boundary.tileKey),
      );
    }
    root.append(group);
  }

  const topology = svgElement("g", {
    class: "pngr-topology",
    "aria-label": "Graph topology",
  });
  for (const edge of layout.edges) {
    topology.append(
      svgElement("path", {
        d: edge.path,
        class: `pngr-edge pngr-edge--${edge.classification}`,
        "data-edge-key": edge.key,
      }),
    );
  }
  root.append(topology);

  const patterns = svgElement("g", {
    class: "pngr-patterns",
    "aria-label": "Anonymous local traversal patterns",
  });
  for (const [index, pattern] of layout.patterns.entries()) {
    const selected = options.selectedPatternId === pattern.id;
    const muted = options.selectedPatternId !== undefined && !selected;
    const selectedThickness = emphasizedThickness(pattern.thickness);
    const color = PATTERN_COLORS[index % PATTERN_COLORS.length] ?? "#a855f7";
    const hitPath = svgElement("path", {
      d: pattern.path,
      class: "pngr-pattern-hit",
      stroke: "transparent",
      "stroke-width": Math.max(12, selectedThickness + 6),
      tabindex: 0,
      role: "button",
      "aria-label": `${pattern.id}, anonymous tile-local pattern, weight ${pattern.weight.toString()}`,
      "data-pattern-id": pattern.id,
    });
    const path = svgElement("path", {
      d: pattern.path,
      class: `pngr-pattern${selected ? " is-selected" : ""}${muted ? " is-muted" : ""}`,
      stroke: color,
      "stroke-width": selected ? selectedThickness : pattern.thickness,
      "aria-hidden": "true",
    });
    const select = (): void => options.onPatternSelect?.(pattern.id);
    const selectWithKeyboard = (event: KeyboardEvent): void => {
      if (event.key === "Enter" || event.key === " ") select();
    };
    hitPath.addEventListener("click", select);
    hitPath.addEventListener("keydown", selectWithKeyboard);
    listeners.push(() => {
      hitPath.removeEventListener("click", select);
      hitPath.removeEventListener("keydown", selectWithKeyboard);
    });
    patterns.append(hitPath, path);
    const label = svgText(
      pattern.labelX,
      pattern.labelY,
      `${pattern.id} × ${pattern.weight.toString()}`,
    );
    label.setAttribute(
      "class",
      `pngr-pattern-label${selected ? " is-selected" : ""}${muted ? " is-muted" : ""}`,
    );
    label.setAttribute("fill", color);
    patterns.append(label);
  }
  root.append(patterns);

  const nodes = svgElement("g", {
    class: "pngr-nodes",
    "aria-label": "Sequence nodes",
  });
  for (const node of layout.nodes) {
    const selected = options.selectedNodeKey === node.key;
    const group = svgElement("g", {
      class: `pngr-node${node.reference ? " pngr-node--reference" : " pngr-node--alternate"}${node.reverse ? " pngr-node--reverse" : ""}${selected ? " is-selected" : ""}`,
      transform: `translate(${node.x} ${node.y - node.height / 2})`,
      tabindex: 0,
      role: "button",
      "aria-label": `Node ${node.ariaLabel}${node.reverse ? ", reverse orientation" : ""}`,
      "data-node-key": node.key,
    });
    const path = node.reverse
      ? `M 7 0 H ${node.width} V ${node.height} H 7 L 0 ${node.height / 2} Z`
      : `M 0 0 H ${node.width - 7} L ${node.width} ${node.height / 2} L ${node.width - 7} ${node.height} H 0 Z`;
    group.append(svgElement("path", { d: path, class: "pngr-node-shape" }));
    if (node.showLabel) {
      const label = svgText(
        node.width / 2,
        node.height / 2 + 4,
        node.showSequence ? node.sequence : node.label,
      );
      label.setAttribute("class", "pngr-node-label");
      label.setAttribute("text-anchor", "middle");
      label.setAttribute("font-size", String(node.labelFontSize));
      label.setAttribute(
        "stroke-width",
        String(node.labelFontSize >= 9 ? 2.5 : 1.5),
      );
      label.setAttribute("data-label-mode", node.labelMode);
      group.append(label);
    }
    const select = (): void => {
      options.onNodeSelect?.(node.key);
    };
    group.addEventListener("click", select);
    group.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") select();
    });
    listeners.push(() => group.removeEventListener("click", select));
    nodes.append(group);
  }
  root.append(nodes);

  const patternPorts = svgElement("g", {
    class: "pngr-pattern-ports",
    "aria-hidden": "true",
  });
  for (const [index, pattern] of layout.patterns.entries()) {
    const selected = options.selectedPatternId === pattern.id;
    const muted = options.selectedPatternId !== undefined && !selected;
    const selectedThickness = emphasizedThickness(pattern.portThickness);
    patternPorts.append(
      svgElement("path", {
        d: pattern.portPath,
        class: `pngr-pattern-port${selected ? " is-selected" : ""}${muted ? " is-muted" : ""}`,
        stroke: PATTERN_COLORS[index % PATTERN_COLORS.length] ?? "#a855f7",
        "stroke-width": selected ? selectedThickness : pattern.portThickness,
        "data-pattern-port-id": pattern.id,
      }),
    );
  }
  root.append(patternPorts);

  const referenceLabel = svgText(18, layout.referenceY + 4, "reference");
  referenceLabel.setAttribute("class", "pngr-reference-label");
  root.append(referenceLabel);
  return {
    svgElements: svg.querySelectorAll("*").length,
    destroy() {
      for (const remove of listeners) remove();
      svg.replaceChildren();
    },
  };
}

function emphasizedThickness(thickness: number): number {
  return thickness + Math.max(1.1, thickness * 0.4);
}

function svgElement<K extends keyof SVGElementTagNameMap>(
  name: K,
  attributes: Readonly<Record<string, string | number>>,
): SVGElementTagNameMap[K] {
  const element = document.createElementNS(SVG_NS, name);
  for (const [key, value] of Object.entries(attributes)) {
    element.setAttribute(key, String(value));
  }
  return element;
}

function svgText(x: number, y: number, value: string): SVGTextElement {
  const element = svgElement("text", { x, y });
  element.textContent = value;
  return element;
}
