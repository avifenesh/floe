/** Direction-aware slide transition between workspace sub-views.
 *
 *  RFC: "Slide transitions between views. Direction-aware. Animation
 *  replays on every switch." Wraps a view keyed by the sub-tab id;
 *  on key change, the outgoing view slides off, the new one slides in.
 *
 *  Direction is inferred from caller via the `order` prop — higher
 *  `order` → slide from right; lower → slide from left. Keeps the
 *  reviewer's spatial model stable across view jumps.
 */

import { useEffect, useRef, useState } from "react";

interface Props {
  /** Stable id for the current view (e.g. sub-tab name). Changes
   *  trigger a replay of the transition. */
  viewKey: string;
  /** Order of this view in the sub-tab sequence. Used to pick slide
   *  direction — right/left or left/right. */
  order: number;
  children: React.ReactNode;
}

export function SlideSwitch({ viewKey, order, children }: Props) {
  const [state, setState] = useState<{ key: string; order: number; dir: "right" | "left" | "none" }>({
    key: viewKey,
    order,
    dir: "none",
  });
  const mountKey = useRef(0);
  useEffect(() => {
    if (state.key === viewKey) return;
    mountKey.current += 1;
    setState({
      key: viewKey,
      order,
      dir: order > state.order ? "right" : order < state.order ? "left" : "none",
    });
  }, [viewKey, order, state.key, state.order]);

  const startClass =
    state.dir === "right"
      ? "adr-slide-from-right"
      : state.dir === "left"
        ? "adr-slide-from-left"
        : "adr-slide-fade";

  return (
    <div
      key={mountKey.current}
      className={startClass + " will-change-transform"}
    >
      {children}
    </div>
  );
}
