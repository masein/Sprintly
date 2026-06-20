// The shipped app version, sourced from package.json so the UI never drifts
// from the released build (QA F12: the landing page hardcoded an old version).
import pkg from "../package.json";

export const APP_VERSION: string = pkg.version;
