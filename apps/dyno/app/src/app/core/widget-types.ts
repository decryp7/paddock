import type {
  DashboardKind,
  DashboardWidget,
  DynoSurface,
  LayoutView,
  MenuBarItem,
  MenuBarKind,
  PlacementState,
  PopoverKind,
  PopoverValue,
  PopoverWidget,
  PopoverSurfaceValues,
  WidgetActionId,
} from './generated/widgets';

/** The popup's layout as the engine returns it. */
export type PopoverView = LayoutView<DynoSurface, PopoverWidget, PopoverKind>;
/** The dashboard's layout. */
export type DashboardView = LayoutView<DynoSurface, DashboardWidget, DashboardKind>;
/** The menu-bar items' layout. */
export type MenuBarView = LayoutView<DynoSurface, MenuBarItem, MenuBarKind>;
/** Everything the popup shows: the notice (and what it is about) and each placement's state. */
export type { PopoverSurfaceValues };
/** One popup widget's state. */
export type PopoverState = PlacementState<PopoverValue, WidgetActionId>;
