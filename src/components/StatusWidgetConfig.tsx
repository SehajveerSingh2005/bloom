import { useState, useEffect, useCallback, useRef } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  useDroppable,
  useDraggable,
  rectIntersection,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  horizontalListSortingStrategy,
  useSortable,
  arrayMove,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  CloudSun,
  BatteryFull,
  Cpu,
  MemoryStick,
  HardDrive,
  ArrowUpDown,
  X,
  ArrowLeftRight,
  ChevronUp,
  ChevronDown,
} from "lucide-react";
import type { ComponentType, SVGProps } from "react";

export interface WidgetConfig {
  left: string[];
  right: string[];
}

interface WidgetDef {
  id: string;
  label: string;
  icon: ComponentType<SVGProps<SVGSVGElement> & { size?: number; strokeWidth?: number }>;
  color: string;
}

const WIDGET_DEFS: WidgetDef[] = [
  { id: "weather", label: "Weather", icon: CloudSun, color: "#60a5fa" },
  { id: "battery", label: "Battery", icon: BatteryFull, color: "#4ade80" },
  { id: "cpu", label: "CPU", icon: Cpu, color: "#f97316" },
  { id: "ram", label: "RAM", icon: MemoryStick, color: "#a78bfa" },
  { id: "disk", label: "Disk", icon: HardDrive, color: "#38bdf8" },
  { id: "net", label: "Net", icon: ArrowUpDown, color: "#2dd4bf" },
];

const DEFAULT_CONFIG: WidgetConfig = {
  left: ["weather"],
  right: ["battery"],
};

const MAX_PER_ZONE = 2;

/* ── Draggable pool chip ── */
function PoolChip({ id }: { id: string }) {
  const def = WIDGET_DEFS.find((w) => w.id === id)!;
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id });
  const Icon = def.icon;
  return (
    <div
      ref={setNodeRef}
      className={`widget-pill widget-pill--available ${isDragging ? "dragging" : ""}`}
      {...listeners}
      {...attributes}
    >
      <Icon size={12} strokeWidth={2} style={{ color: def.color }} />
      <span>{def.label}</span>
    </div>
  );
}

/* ── Sortable placed chip ── */
function SortablePlacedChip({
  id,
  idx,
  total,
  onSwap,
  onRemove,
  onMove,
}: {
  id: string;
  idx: number;
  total: number;
  onSwap: (id: string) => void;
  onRemove: (id: string) => void;
  onMove: (id: string, dir: -1 | 1) => void;
}) {
  const def = WIDGET_DEFS.find((w) => w.id === id)!;
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.4 : 1,
    zIndex: isDragging ? 50 : ("auto" as const),
  };

  const Icon = def.icon;

  return (
    <div ref={setNodeRef} style={style} className={`widget-pill widget-pill--placed ${isDragging ? "dragging" : ""}`}>
      <div {...attributes} {...listeners} className="widget-pill-drag-handle">
        <Icon size={12} strokeWidth={2} style={{ color: def.color }} />
        <span>{def.label}</span>
      </div>
      <div className="widget-pill-btns">
        {idx > 0 && (
          <button className="widget-pill-btn" onClick={() => onMove(id, -1)}>
            <ChevronUp size={9} />
          </button>
        )}
        {idx < total - 1 && (
          <button className="widget-pill-btn" onClick={() => onMove(id, 1)}>
            <ChevronDown size={9} />
          </button>
        )}
        <button className="widget-pill-btn" title="Swap side" onClick={() => onSwap(id)}>
          <ArrowLeftRight size={9} />
        </button>
        <button className="widget-pill-btn widget-pill-btn--x" title="Remove" onClick={() => onRemove(id)}>
          <X size={10} />
        </button>
      </div>
    </div>
  );
}

/* ── Droppable zone ── */
function DropZone({
  id,
  side,
  items,
  onSwap,
  onRemove,
  onMove,
}: {
  id: string;
  side: "left" | "right";
  items: string[];
  onSwap: (id: string) => void;
  onRemove: (id: string) => void;
  onMove: (id: string, dir: -1 | 1) => void;
}) {
  const { setNodeRef, isOver } = useDroppable({ id });
  return (
    <div
      ref={setNodeRef}
      className={`widget-config-zone ${isOver ? "widget-config-zone--over" : ""}`}
    >
      <span className="widget-config-side">{side === "left" ? "Left" : "Right"}</span>
      <div className="widget-config-chips">
        <SortableContext items={items} strategy={horizontalListSortingStrategy}>
          {items.length > 0
            ? items.map((itemId, i) => (
                <SortablePlacedChip
                  key={itemId}
                  id={itemId}
                  idx={i}
                  total={items.length}
                  onSwap={onSwap}
                  onRemove={onRemove}
                  onMove={onMove}
                />
              ))
            : <span className="widget-config-empty">Drop here</span>}
        </SortableContext>
      </div>
    </div>
  );
}

/* ── Main component ── */
interface StatusWidgetConfigProps {
  value: WidgetConfig;
  onChange: (config: WidgetConfig) => void;
}

export function StatusWidgetConfig({ value, onChange }: StatusWidgetConfigProps) {
  const [config, setConfig] = useState<WidgetConfig>(() => value || DEFAULT_CONFIG);
  const [activeId, setActiveId] = useState<string | null>(null);
  const draggedFromZone = useRef<"left" | "right" | null>(null);

  useEffect(() => {
    setConfig(value || DEFAULT_CONFIG);
  }, [value]);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } })
  );

  const allPlaced = new Set([...config.left, ...config.right]);
  const pool = WIDGET_DEFS.filter((w) => !allPlaced.has(w.id)).map((w) => w.id);

  const emit = (next: WidgetConfig) => {
    setConfig(next);
    onChange(next);
  };

  const remove = (id: string) => {
    emit({
      left: config.left.filter((x) => x !== id),
      right: config.right.filter((x) => x !== id),
    });
  };

  const swapSide = (id: string) => {
    if (config.left.includes(id)) {
      emit({ left: config.left.filter((x) => x !== id), right: [...config.right, id] });
    } else {
      emit({ right: config.right.filter((x) => x !== id), left: [...config.left, id] });
    }
  };

  const moveInSide = (id: string, dir: -1 | 1) => {
    for (const side of ["left", "right"] as const) {
      const arr = config[side];
      const idx = arr.indexOf(id);
      if (idx === -1) continue;
      const swap = idx + dir;
      if (swap < 0 || swap >= arr.length) return;
      emit({ ...config, [side]: arrayMove(arr, idx, swap) });
      return;
    }
  };

  const getZoneOf = (itemId: string): "left" | "right" | "pool" | null => {
    if (config.left.includes(itemId)) return "left";
    if (config.right.includes(itemId)) return "right";
    if (pool.includes(itemId)) return "pool";
    return null;
  };

  const handleDragStart = useCallback((event: DragStartEvent) => {
    const id = event.active.id as string;
    setActiveId(id);
    const zone = getZoneOf(id);
    draggedFromZone.current = zone === "pool" ? null : (zone as "left" | "right" | null);
  }, [config, pool]);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      setActiveId(null);
      if (!over) return;

      const itemId = active.id as string;
      const overId = over.id as string;
      const sourceZone = draggedFromZone.current;
      const isPoolItem = pool.includes(itemId);

      // Determine target zone
      let targetZone: "left" | "right" | null = null;
      if (overId === "zone-left" || overId === "zone-right") {
        targetZone = overId === "zone-left" ? "left" : "right";
      } else if (config.left.includes(overId)) {
        targetZone = "left";
      } else if (config.right.includes(overId)) {
        targetZone = "right";
      }

      // Pool item → add to zone
      if (isPoolItem && targetZone) {
        if (config[targetZone].length >= MAX_PER_ZONE) return;
        emit({ ...config, [targetZone]: [...config[targetZone], itemId] });
        return;
      }

      // Zone item → reorder or move
      if (sourceZone && targetZone) {
        if (sourceZone === targetZone) {
          // Reorder within zone
          const arr = config[sourceZone];
          const oldIdx = arr.indexOf(itemId);
          const newIdx = arr.indexOf(overId);
          if (oldIdx !== -1 && newIdx !== -1 && oldIdx !== newIdx) {
            emit({ ...config, [sourceZone]: arrayMove(arr, oldIdx, newIdx) });
          }
        } else {
          // Move between zones
          if (config[targetZone].length >= MAX_PER_ZONE) return;
          const newSource = config[sourceZone].filter((x) => x !== itemId);
          const targetArr = [...config[targetZone]];
          const insertIdx = targetArr.indexOf(overId);
          if (insertIdx >= 0) {
            targetArr.splice(insertIdx, 0, itemId);
          } else {
            targetArr.push(itemId);
          }
          emit({ ...config, [sourceZone]: newSource, [targetZone]: targetArr });
        }
        return;
      }

      // Pool item missed all zones — check pointer overlap for empty zones
      if (isPoolItem && !targetZone && active.rect.current.initial) {
        const rect = active.rect.current.initial;
        const delta = event.delta;
        const cx = rect.left + rect.width / 2 + delta.x;
        const cy = rect.top + rect.height / 2 + delta.y;

        for (const side of ["left", "right"] as const) {
          if (config[side].length >= MAX_PER_ZONE) continue;
          const el = document.getElementById(`zone-${side}`);
          if (!el) continue;
          const r = el.getBoundingClientRect();
          if (cx >= r.left && cx <= r.right && cy >= r.top && cy <= r.bottom) {
            emit({ ...config, [side]: [...config[side], itemId] });
            return;
          }
        }
      }
    },
    [config, pool]
  );

  const activeDef = activeId ? WIDGET_DEFS.find((w) => w.id === activeId) : null;

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={rectIntersection}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
    >
      <div className="widget-config">
        <DropZone
          id="zone-left"
          side="left"
          items={config.left}
          onSwap={swapSide}
          onRemove={remove}
          onMove={moveInSide}
        />
        <DropZone
          id="zone-right"
          side="right"
          items={config.right}
          onSwap={swapSide}
          onRemove={remove}
          onMove={moveInSide}
        />

        {pool.length > 0 && (
          <div className="widget-config-pool">
            <div className="widget-config-pool-chips">
              {pool.map((id) => (
                <PoolChip key={id} id={id} />
              ))}
            </div>
          </div>
        )}
      </div>

      <DragOverlay>
        {activeDef ? (
          <div className="widget-pill widget-pill--dragging">
            <activeDef.icon size={12} strokeWidth={2} style={{ color: activeDef.color }} />
            <span>{activeDef.label}</span>
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}
