import { useState, type ReactNode } from 'react';
import { motion, AnimatePresence, useDragControls } from 'framer-motion';

interface WindowProps {
  id: string;
  title: string;
  isOpen: boolean;
  isFocused: boolean;
  isMinimized: boolean;
  onClose: () => void;
  onMinimize: () => void;
  onFocus: () => void;
  children: ReactNode;
  width?: number;
  height?: number;
  defaultPosition?: { x: number; y: number };
  viewport?: { w: number; h: number };
}

export default function Window({
  id,
  title,
  isOpen,
  isFocused,
  isMinimized,
  onClose,
  onMinimize,
  onFocus,
  children,
  width = 680,
  height = 440,
  defaultPosition = { x: 80, y: 120 },
  viewport = { w: 1440, h: 900 },
}: WindowProps) {
  const [isMaximized, setIsMaximized] = useState(false);
  const dragControls = useDragControls();

  if (!isOpen) return null;

  // Clamp position so window never leaves the viewport
  const clampedX = Math.max(16, Math.min(defaultPosition.x, viewport.w - width - 16));
  const clampedY = Math.max(56, Math.min(defaultPosition.y, viewport.h - height - 80));

  return (
    <AnimatePresence>
      {!isMinimized && (
        <motion.div
          id={`window-${id}`}
          onPointerDown={onFocus}
          drag={!isMaximized}
          dragControls={dragControls}
          dragListener={false}
          dragMomentum={false}
          dragElastic={0.05}
          dragConstraints={{
            left: 16,
            right: Math.max(16, viewport.w - 100),
            top: 56,
            bottom: Math.max(56, viewport.h - 100),
          }}
          initial={{ scale: 0.92, opacity: 0, x: clampedX, y: clampedY + 30 }}
          animate={{
            scale: 1,
            opacity: 1,
            x: isMaximized ? 0 : clampedX,
            y: isMaximized ? 0 : clampedY,
            width: isMaximized ? viewport.w - 48 : width,   // subtract px-6 * 2
            height: isMaximized ? viewport.h - 136 : height, // subtract pt-14 + pb-20
            zIndex: isFocused ? 50 : 20,
          }}
          exit={{ scale: 0.92, opacity: 0, transition: { duration: 0.15 } }}
          transition={{ type: 'spring', stiffness: 450, damping: 28 }}
          style={{
            position: 'absolute',
            display: 'flex',
            flexDirection: 'column',
            borderRadius: '12px',
            overflow: 'hidden',
            border: '1px solid rgba(255,255,255,0.08)',
            boxShadow: '0 12px 40px 0 rgba(0,0,0,0.5)',
            background: 'rgba(10, 10, 15, 0.75)',
            backdropFilter: 'blur(25px) saturate(140%)',
            WebkitBackdropFilter: 'blur(25px) saturate(140%)',
            transformOrigin: 'center center',
          }}
        >
          <div
            onPointerDown={(e) => dragControls.start(e)}
            className="flex items-center justify-between pl-4 pr-1 py-1.5 bg-white/[0.02] border-b border-white/[0.05] cursor-grab active:cursor-grabbing select-none"
          >
            <span className="text-[11px] font-semibold tracking-wide text-white/55 select-none">
              {title}
            </span>
            <div className="flex items-center">
              <button
                onClick={(e) => { e.stopPropagation(); onMinimize(); }}
                className="w-10 h-8 flex items-center justify-center text-white/40 hover:text-white hover:bg-white/10 transition-colors"
                title="Minimize"
              >
                <svg width="10" height="1" viewBox="0 0 10 1" fill="none" stroke="currentColor" strokeWidth="1.2">
                  <line x1="0" y1="0.5" x2="10" y2="0.5" />
                </svg>
              </button>
              <button
                onClick={(e) => { e.stopPropagation(); setIsMaximized(!isMaximized); }}
                className="w-10 h-8 flex items-center justify-center text-white/40 hover:text-white hover:bg-white/10 transition-colors"
                title={isMaximized ? 'Restore' : 'Maximize'}
              >
                {isMaximized ? (
                  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2">
                    <path d="M2.5 2.5V0.5H9.5V7.5H7.5M0.5 2.5H7.5V9.5H0.5V2.5Z" />
                  </svg>
                ) : (
                  <svg width="9" height="9" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2">
                    <rect x="0.5" y="0.5" width="9" height="9" />
                  </svg>
                )}
              </button>
              <button
                onClick={(e) => { e.stopPropagation(); onClose(); }}
                className="w-10 h-8 flex items-center justify-center text-white/40 hover:text-white hover:bg-[#ff3b30] transition-colors"
                title="Close"
              >
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2">
                  <path d="M1 1L9 9M9 1L1 9" />
                </svg>
              </button>
            </div>
          </div>

          <div className="flex-1 overflow-auto bg-black/[0.1] relative">
            {children}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
