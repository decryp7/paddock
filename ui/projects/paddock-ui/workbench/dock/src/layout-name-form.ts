import { linkedSignal, type WritableSignal } from '@angular/core';
import { form, validate, type FieldTree } from '@angular/forms/signals';
import { layoutNameProblem, MAX_LAYOUT_NAME, type LayoutNameProblem } from './named-layouts';

/** The words for each way a layout name can be refused. */
export function layoutNameMessage(problem: LayoutNameProblem): string {
  switch (problem) {
    case 'empty':
      return $localize`:@@pd.wb.dock.layout.name-empty:Give the layout a name.`;
    case 'duplicate':
      return $localize`:@@pd.wb.dock.layout.name-duplicate:A layout with this name exists. Pick another name.`;
    case 'too-long':
      return $localize`:@@pd.wb.dock.layout.name-too-long:Use at most ${MAX_LAYOUT_NAME}:max: characters.`;
  }
}

/** A Signal Forms form of one layout name, validated for empty, duplicate and too long (`layoutNameProblem`). */
export interface LayoutNameForm {
  /** The form's value; it starts again from `initial()` whenever that changes. */
  readonly model: WritableSignal<{ name: string }>;
  readonly name: FieldTree<string>;
}

/**
 * The form of a layout name for a dialog (an injection context). `taken` are the names in use, `own` the one being renamed
 * (it may keep its name). Used by the save dialog and by Rename in the manage dialog.
 */
export function layoutNameForm(initial: () => string, taken: () => readonly string[], own: () => string = () => ''): LayoutNameForm {
  const model = linkedSignal(() => ({ name: initial() }));
  const f = form(model, (path) => {
    validate(path.name, ({ value }) => {
      const problem = layoutNameProblem(value(), taken(), own());
      return problem ? { kind: problem, message: layoutNameMessage(problem) } : undefined;
    });
  });
  return { model, name: f.name };
}
