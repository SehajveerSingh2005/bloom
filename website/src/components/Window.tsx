import { useState, useEffect, type ReactNode } from 'react';
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
  width?: string;
  height?: string;
  defaultPosition?: { x: number; y: number };
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
  width = 'w-[680px]',
  height = 'h-[440px]',
  defaultPosition = { x: 80, y: 120 },
}: WindowProps) {
  const [isMaximized, setIsMaximized] = useState(false);
  const dragControls = useDragControls();

  const [windowSize, setWindowSize] = useState({
    width: typeof window !== 'undefined' ? window.innerWidth : 1200,
    height: typeof window !== 'undefined' ? window.innerHeight : 800,
  });

  useEffect(() => {
    const handleResize = () => {
      setWindowSize({ width: window.innerWidth, height: window.innerHeight });
    };
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  if (!isOpen) return null;

  return (
    <AnimatePresence>
      {!isMinimized && (
        <motion.div
          id={`window-${id}`}
          onClick={onFocus}
          drag={!isMaximized}
          dragControls={dragControls}
          dragListener={false}
          dragMomentum={false}
          dragElastic={0.05}
          dragConstraints={{ left: -300, right: windowSize.width - 200, top: 0, bottom: windowSize.height - 150 }}
          onDragEnd={() => {
            const el = document.getElementById(`window-${id}`);
            if (el) {
              const style = window.getComputedStyle(el);
              const matrix = new DOMMatrix(style.transform);
              console.log(`Window "${id}" dragged to: x: ${Math.round(matrix.m41)}, y: ${Math.round(matrix.m42)}`);
            }
          }}
          initial={{ scale: 0.92, opacity: 0, y: defaultPosition.y + 30, x: defaultPosition.x }}
          animate={{
            scale: 1,
            opacity: 1,
            x: isMaximized ? 0 : undefined,
            y: isMaximized ? 0 : undefined,
            width: isMaximized ? '100vw' : undefined,
            height: isMaximized ? 'calc(100vh - 48px)' : undefined, // Subtract taskbar/notch room
            top: isMaximized ? '48px' : undefined,
            left: isMaximized ? '0px' : undefined,
            zIndex: isFocused ? 50 : 20,
          }}
          exit={{
            scale: 0.92,
            opacity: 0,
            transition: { duration: 0.15 }
          }}
          transition={{ type: 'spring', stiffness: 450, damping: 28 }}
          className={`absolute flex flex-col glass-panel-dark rounded-xl overflow-hidden border border-white/8 shadow-2xl max-w-[calc(100vw-32px)] max-h-[calc(100vh-80px)] ${
            isMaximized ? '' : `${width} ${height}`
          }`}
          style={{
            transformOrigin: 'center center',
          }}
        >
          {/* Header/Titlebar */}
          <div 
            onPointerDown={(e) => dragControls.start(e)}
            className="window-titlebar flex items-center justify-between px-4 py-3 bg-white/[0.02] border-b border-white/[0.05] cursor-grab active:cursor-grabbing select-none"
          >
            {/* Control buttons */}
            <div className="flex items-center gap-2">
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onClose();
                }}
                className="w-3 h-3 rounded-full bg-[#ff5f56] hover:bg-[#ff3b30] flex items-center justify-center border border-black/10 transition-colors group relative"
                title="Close"
              >
                <span className="text-[7px] text-black/50 opacity-0 group-hover:opacity-100 font-bold select-none absolute">×</span>
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onMinimize();
                }}
                className="w-3 h-3 rounded-full bg-[#ffbd2e] hover:bg-[#ff9500] flex items-center justify-center border border-black/10 transition-colors group relative"
                title="Minimize"
              >
                <span className="text-[7px] text-black/50 opacity-0 group-hover:opacity-100 font-bold select-none absolute">−</span>
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setIsMaximized(!isMaximized);
                }}
                className="w-3 h-3 rounded-full bg-[#27c93f] hover:bg-[#34c759] flex items-center justify-center border border-black/10 transition-colors group relative"
                title={isMaximized ? "Restore" : "Maximize"}
              >
                <span className="text-[5px] text-black/50 opacity-0 group-hover:opacity-100 font-bold select-none absolute">⤢</span>
              </button>
            </div>

            {/* Title */}
            <span className="text-[12px] font-medium tracking-wide text-white/50 text-center flex-1 pr-14 select-none">
              {title}
            </span>
          </div>

          {/* Window Client Area */}
          <div className="flex-1 overflow-auto bg-black/[0.1] relative">
            {children}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
