import type { Edit, LayoutView, SurfaceValues, WidgetState } from '@decryp7/paddock-ui/widgets';
import type { PitwallSurface, PopoverHeader, PopoverValue, PopoverWidget, WidgetActionId } from './generated/widgets';

/** The tray popup's layout as the chooser draws it (the generic types are the shared package's, ADR 0035). */
export type PopoverView = LayoutView<PitwallSurface, PopoverWidget, PopoverWidget>;
/** One edit of the popup's layout. */
export type PopoverEdit = Edit<PopoverWidget>;
/** What the popup's widgets show now, computed by the engine. */
export type PopoverValues = SurfaceValues<PopoverValue, WidgetActionId>;
/** One widget's state. */
export type PopoverState = WidgetState<PopoverValue, WidgetActionId>;

/**
 * What the app pushes to its windows on the event `pitwall://popover` after any change: the layout, what the widgets show and
 * the header. Mirrors `PopoverPush` in `app/src-tauri/src/popup.rs` (a test there pins its three keys).
 */
export interface PopoverPush {
  readonly view: PopoverView;
  readonly values: PopoverValues;
  readonly header: PopoverHeader;
}
