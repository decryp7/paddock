import { DIALOG_KIND_TABLE, DIALOG_WIDTH_STEPS, type DialogContractData, type DialogKind, type DialogModality, type DialogWidthStep } from '@decryp7/paddock-ui/shell';

/** What a spec states of a made-up dialog's contract; the rest comes from the kind table, as Rust fills it. */
export interface FakeContractSpec<Id extends string, Cmd extends string> {
  readonly id: Id;
  readonly kind: DialogKind;
  /** Only a form's is needed; the other kinds decide. */
  readonly modality?: DialogModality;
  /** A width step, or a tool's number. */
  readonly width?: DialogWidthStep | number;
  readonly height?: number | null;
  readonly instanced?: boolean;
  readonly requiredChoice?: boolean;
  readonly commands?: readonly Cmd[];
}

/**
 * A dialog contract for a spec's made-up dialog, filled by the generated kind table the way `paddock_dialogs::dialogs!`
 * fills it (ADR 0026 §3). Tests only: an app's dialogs take the contract Rust generated (`DIALOG_CONTRACTS['<id>']`).
 */
export function fakeDialogContract<const Id extends string, const Cmd extends string = never>(spec: FakeContractSpec<Id, Cmd>): DialogContractData<Id, Cmd> {
  const table = DIALOG_KIND_TABLE[spec.kind];
  const modality = spec.modality ?? (table.modalities[0] as DialogModality);
  const step = typeof spec.width === 'string' ? spec.width : spec.width === undefined && spec.kind !== 'tool' ? (Object.entries(DIALOG_WIDTH_STEPS).find(([, px]) => px === table.width)?.[0] as DialogWidthStep) : null;
  const width = typeof spec.width === 'number' ? spec.width : step ? DIALOG_WIDTH_STEPS[step] : table.width;
  return {
    id: spec.id,
    kind: spec.kind,
    modality,
    width,
    widthStep: step ?? null,
    height: spec.height === undefined ? table.height : spec.height,
    instanced: spec.instanced ?? false,
    requiredChoice: spec.requiredChoice ?? false,
    commands: spec.commands ?? [],
  };
}
