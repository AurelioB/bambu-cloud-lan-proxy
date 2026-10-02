# Printer console redesign

## Direction and comparison

The console is a connection manager, not a replacement for a slicer or print-farm application. Its main questions are: which printers are available, what works on each printer, and what does an app need to connect?

| Reference | Relevant strengths | Decision for this console |
| --- | --- | --- |
| [Bambuddy](https://github.com/maziggy/bambuddy) | Multi-printer monitoring and a self-hosted command center | Use scannable printer cards, a compact list alternative, and capability-aware state. Leave job libraries and print controls in connected apps. |
| [Mainsail](https://github.com/mainsail-crew/mainsail) | Responsive printer dashboard, configurable panels, theming, multi-printer support | Make navigation predictable on desktop and mobile; keep connection detail in a side sheet so it does not overwhelm the overview. |
| [SimplyPrint](https://simplyprint.io/features) | Fleet overview, printer organization, monitoring, queue management | Prioritize search and attention filtering as the collection grows. Do not introduce a queue or analytics without supporting backend data. |

These are design interpretations, not feature-equivalence claims. No code, imagery, or brand assets were copied from the reference projects.

The resulting visual direction uses warm neutral surfaces, forest-green accents, generous spacing, compact service summaries, and an original generic printer schematic. The schematic is decorative, not a photograph or a claim about a model's enclosure. Printer names and models come from the API; there is no H2C-specific production UI.

## Theme and composition

The [shadcn-svelte theming approach](https://shadcn-svelte.com/docs/theming) controls component colors through semantic OKLCH variables in `web/src/app.css`. The existing Vega component style is preserved. Success, warning, and editorial surface tokens extend the standard theme, with Badge variants for status. Local Inter font assets avoid a runtime font CDN.

Light, dark, and system appearance use [mode-watcher as recommended by shadcn-svelte](https://shadcn-svelte.com/docs/dark-mode/svelte). Controls retain library variants and focus treatments; custom CSS handles product layout and original illustration rather than recoloring individual controls.

Four pages:

- **Printers:** online, local-endpoint, and attention counts; name/model/serial/address search; filter and card/list views.
- **Connections:** per-printer addresses, standard ports, public CA download, and setup explanations.
- **Bambu account:** connection summary and the existing two-step cloud sign-in.
- **Permissions:** explicit draft, reset, and save for command level and file/printing permissions.

Connection editing uses a Sheet scoped to one printer. Keyboard focus returns to its originating Configure button. Polling does not replace an active edit. Save failures retain the draft. Local networking application remains an explicit host operation; saving an address does not claim to apply it.

## Entire component catalog review

Reviewed all **62 entries** in the [official component catalog](https://shadcn-svelte.com/docs/components), including entries not installed in this project. “Deferred” means reviewed but not justified by a current workflow, not a promised feature. The registry supplies the installed primitives; unused alternatives were not installed just to fill a checklist.

| Component(s) | Disposition and functional reason |
| --- | --- |
| Accordion | Used for optional setup explanations. |
| Alert | Used for actionable errors and networking instructions. |
| Alert Dialog | Deferred: this redesign does not directly delete files or start/cancel prints. Add for an actual destructive action. |
| Aspect Ratio | Deferred: the decorative SVG uses its own viewBox; no camera/video viewer is introduced. |
| Attachment | Deferred: no upload composer in this admin console. |
| Avatar | Deferred: no profile-image API or user switcher. |
| Badge | Used for states and capabilities; semantic success/warning variants. |
| Breadcrumb | Deferred: navigation is flat, not hierarchical; the header only identifies the current section. |
| Bubble | Deferred: no messaging workflow. |
| Button | Used for actions with built-in default, outline, ghost, and size variants. |
| Button Group | Deferred: mutually exclusive view/filter choices need Toggle Group semantics. |
| Calendar | Deferred: no scheduling workflow. |
| Card | Used for printers and related settings sections with header/content/footer composition. |
| Carousel | Deferred: printer availability should be visible, not hidden in a carousel. |
| Chart | Deferred: the overview API has no time-series metrics. |
| Checkbox | Deferred: persisted boolean permissions use Switch with explicit Save. |
| Collapsible | Deferred: grouped help uses Accordion; adding another disclosure pattern is unnecessary. |
| Combobox | Deferred: free-text filtering is sufficient; no large option-selection field. |
| Command | Deferred: four navigation destinations do not justify a command palette. |
| Context Menu | Deferred: important actions remain visible and usable on touch devices. |
| Data Table | Deferred: basic Table plus search/filter meets current needs without a sorting/pagination engine. |
| Date Picker | Deferred: no date selection. |
| Dialog | Deferred as a separate product pattern: Sheet provides focused per-printer editing using dialog semantics. |
| Drawer | Deferred: the responsive Sheet works on mobile and desktop without two editor implementations. |
| Dropdown Menu | Used for light/dark/system appearance; items are grouped and labelled. |
| Empty | Used for first-run and zero search results. |
| Field | Used for labelled forms, descriptions, validation, and grouped settings. |
| Hover Card | Deferred: critical connection information is visible, not hover-dependent. |
| Input | Used for access codes, addresses, and the cloud callback URL. |
| Input Group | Used for the search input and search icon. |
| Input OTP | Deferred: the proxy code is a reusable secret, not a one-time verification code. |
| Item | Deferred: structured printer information is already represented by Card/Table. |
| Kbd | Deferred: no new product-wide shortcut map. |
| Label | Used through the Field primitives; no duplicate manual label system. |
| Marker | Deferred: no annotation workflow. |
| Menubar | Deferred: a desktop-style application menu does not fit this admin console. |
| Message | Deferred: no conversation workflow. |
| Native Select | Deferred: no native dropdown field required. |
| Navigation Menu | Deferred: Sidebar better serves a persistent application workspace. |
| Pagination | Deferred: all printers are returned by one small overview response; filtering is client-side. Revisit for large fleets. |
| Popover | Deferred: no small contextual form requires it. |
| Progress | Deferred: no truthful print/upload percentage is exposed by the overview API. |
| Radio Group | Deferred: Toggle Group already provides the small mutually exclusive command-level choices. |
| Range Calendar | Deferred: no date-range workflow. |
| Resizable | Deferred: no multi-pane editor or configurable dashboard layout. |
| Scroll Area | Deferred: native scrolling is sufficient for pages, the Sheet, and tables. |
| Select | Deferred: small option sets remain immediately visible in Toggle Group. |
| Separator | Used to separate card and editor sections. |
| Sheet | Used for one-printer connection editing and responsive sidebar navigation. |
| Sidebar | Used for the four workspace sections, mobile navigation, guide link, and lock action. |
| Skeleton | Used while opening a saved session. |
| Slider | Deferred: no numeric hardware controls in this admin UI. |
| Sonner | Used for save, copy, and download feedback. |
| Spinner | Used inside pending action buttons. |
| Switch | Used for endpoint availability and boolean permissions. |
| Table | Used for dense printer and connection-address views. |
| Tabs | Deferred: primary navigation is now Sidebar rather than a second parallel tab bar. Existing primitive retained. |
| Textarea | Installed as an Input Group registry dependency; no multiline product field currently needs it. |
| Toggle | Retained as a Toggle Group dependency; no standalone ambiguous toggle action. |
| Toggle Group | Used for filtering, card/list layout, and command level. |
| Tooltip | Installed for Sidebar behavior; primary guidance remains visible text. |
| Typography | Applied through semantic headings, descriptions, and the global type scale; no separate registry component needed. |

## Accuracy and capability rules

- Data comes from the overview, settings, endpoint, cloud-auth, and CA APIs. Overview capabilities now include sanitized runtime evidence and per-field provenance.
- Printer online state, local endpoint health, and signing-certificate readiness remain distinct.
- “Camera relay ready” means the relay service health check passed, not that the browser verified live frames.
- Image cameras share the port-6000 native-file listener. Unknown capabilities remain explicitly labelled; runtime AMS evidence does not imply a known camera or toolhead profile.
- Cloud-print permission is independent of the command level, matching existing policy behavior; read-only wording does not pretend to disable that independent path.
- No invented temperatures, print percentages, completion estimates, or charts.
- On transient polling failure, last-known status remains visible with a stale-data warning. Authentication failure clears it.
- A lock invalidates pending overview responses. Printer LAN codes are blank in the UI and only sent when supplied by the user.

## Verification

From `web/`:

```sh
npm ci
npx playwright install chromium
npm run check
npm run build
npm run test:ui
```

The browser suite mocks every API call; it never controls a real printer or signs in to Bambu. Fixtures use reserved documentation IPs, DEMO serials, and a fake access code. It covers mixed capabilities, search/filter/layout, appearance, mobile navigation, empty state, endpoint validation and save isolation, polling-resistant drafts, failed saves, account callback, certificate download, and lock/recovery behavior. Axe checks cover the tested screens in addition to manual screenshot review; passing them is not a claim of complete accessibility conformance.

Screenshots are generated under `web/test-results/`. CI runs the browser suite and retains screenshots/traces as a short-lived artifact. The redesign does not require a new service, production dependency, or Docker Compose topology; the existing frontend build is embedded by the Rust build as before.

This change does not deploy or restart the live proxy.
