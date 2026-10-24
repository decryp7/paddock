// Generated from the Rust wire types of paddock-service (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib generated.

export type Mechanism = "launchdBundled" | "launchdFile" | "systemdUser" | "systemdSystem" | "windowsRun" | "windowsService";

export type Condition = "running" | "repairing" | "notInstalled" | "notRunning" | "refused" | "programMissing" | "moved" | "switchedOff" | "needsApproval" | "unavailable" | "newerOwner";

export type RepairState = "working" | "succeeded" | "failed";

export type RepairNotice = { 
/**
 * Where the repair is.
 */
state: RepairState, 
/**
 * The words to show.
 */
message: string, 
/**
 * The current phase while working.
 */
step: string | null, };

export type ServiceStatus = { 
/**
 * How it is registered.
 */
mechanism: Mechanism, 
/**
 * How it is doing.
 */
condition: Condition, 
/**
 * Something is registered.
 */
installed: boolean, 
/**
 * It answers.
 */
running: boolean, 
/**
 * What is off and what to do, in words.
 */
message: string | null, 
/**
 * Repair applies (re-registers it; may show an OS prompt where [`Capabilities::automatic`] is false).
 */
canRepair: boolean, 
/**
 * Install applies.
 */
canInstall: boolean, 
/**
 * "Open Login Items" applies (the user's switch; the app never flips it).
 */
canOpenLoginItems: boolean, 
/**
 * The repair's progress.
 */
repair: RepairNotice | null, };

export type ServiceErrorCode = "notSetUp" | "busy" | "switchedOff" | "needsApproval" | "needsElevation" | "declined" | "unavailable" | "newerOwner" | "failed";

export type ServiceCommandError = { 
/**
 * What kind.
 */
code: ServiceErrorCode, 
/**
 * Words for the user, with the next step.
 */
message: string, 
/**
 * Whether trying again can help.
 */
retryable: boolean, };

export type ServiceUpdate = { 
/**
 * The service now.
 */
status: ServiceStatus, 
/**
 * Why the request that ended here did not work, if it did not.
 */
error: ServiceCommandError | null, };

/** The event every service update is sent on, to every window. */
export const SERVICE_EVENT = 'paddock://service';

/** The id of the launch-at-login settings section (and of its settings page). */
export const LOGIN_SECTION = 'pd.login';
