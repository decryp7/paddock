/**
 * `@decryp7/paddock-ui/material`: the app-side half of the Material bridge (rule 47, Material first). The look is
 * `css/material.css` (one Material 3 theme mapped to the --pd-* tokens); this entry sets the behaviour defaults that go with
 * it. Needs `@angular/material` (an optional peer of the package).
 */
export { providePaddockMaterial, PADDOCK_FORM_FIELD_OPTIONS, PADDOCK_DATE_FORMATS, PADDOCK_TOOLTIP_OPTIONS } from './src/provide';
