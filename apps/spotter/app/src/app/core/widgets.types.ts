import type { Edit, LayoutView, Span, SurfaceValues, WidgetState } from '@decryp7/paddock-ui/widgets';
import type { PopoverValue, PopoverWidget, SpotterSurface, WidgetActionId } from './generated/widgets';

/** The tray popup's layout as the chooser draws it (the generic types are the shared package's, ADR 0035). */
export type PopoverView = LayoutView<SpotterSurface, PopoverWidget, Span, PopoverWidget>;
/** One edit of the popup's layout. */
export type PopoverEdit = Edit<PopoverWidget>;
/** What the popup's widgets show now, computed by the engine. */
export type PopoverValues = SurfaceValues<PopoverValue, WidgetActionId>;
/** One widget's state. */
export type PopoverState = WidgetState<PopoverValue, WidgetActionId>;
