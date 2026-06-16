// Pre-included example guest programs. Each entry is a self-contained config:
// its built-in module, description, form inputs, orchestration script, and how
// the form maps to the VM run's inputs. Add an entry to EXAMPLES and a tab, its
// form, and its built-in module all appear — no other wiring needed.

import { sudokuForm } from "./sudoku-form";

/// A size preset: fills a text field with `bytes` of deterministic filler so
/// runs are reproducible and the cost scaling is visible.
export interface Preset {
  label: string;
  bytes: number;
}

/// One configurable input on an example's form: a single-line text field, or a
/// multi-line one (`rows` > 1). Add new shapes here as examples need them.
export interface Field {
  name: string; // key in the values map passed to toInputs
  label: string;
  value: string;
  rows?: number; // > 1 renders a textarea
  maxLength?: number;
  presets?: Preset[];
  presetsHint?: string;
}

/// The VM run inputs an example derives from its form. `priv` byte arrays are
/// the prover's secret (the verifier gets them blinded to length); `pub` is
/// shared with both parties and visible to the orchestration script as `pub`.
export interface RunInputs {
  priv: Record<string, Uint8Array>;
  pub: Record<string, unknown>;
}

/// The input region of a form: its root element and a getter for the current
/// values (keyed however `toInputs` expects). Returned by the declarative field
/// renderer or by a custom `ExampleProgram.render`.
export interface FormBody {
  el: HTMLElement;
  values(): Record<string, string>;
}

/// A pre-included example: a built-in module plus the form and orchestration
/// that drive it. Most examples declare `fields`; one with a bespoke UI (e.g.
/// the sudoku grid) supplies `render` instead.
export interface ExampleProgram {
  id: string; // unique; also the tab key
  label: string; // tab button text
  description: string;
  moduleName: string; // display name for the built-in module
  wasmUrl: string; // path under BASE_URL to the built-in .wasm
  script: string; // default orchestration script
  fields?: Field[];
  render?: () => FormBody; // custom input region; overrides `fields`
  toInputs(values: Record<string, string>): RunInputs;
}

const SHA256_SCRIPT = `// Both parties run this exact script against \`vm\` (the zkVM API) and the
// typed \`guest\` API. \`pub\` is public to both; \`priv\` holds the private
// message. Return the value revealed to both parties.

const len = pub.len;

// Reserve message + 32-byte digest space in the guest's linear memory.
const ptr = await vm.callLocal("cabi_realloc", [
  Public(0), Public(0), Public(1), Public(len + 32),
]);

// Stage the private message: the prover contributes the real bytes; the
// verifier blinds by their length.
vm.writePrivate(ptr, priv.message);

// Hash it inside the VM. Only the digest pointer is revealed to both.
const digestPtr = await vm.call("hash", [Public(ptr), Public(len)]);

return helpers.hex(vm.read(digestPtr, 32));
`;

const SUDOKU_SCRIPT = `// The 9×9 puzzle (\`pub.puzzle\`, 81 public cells, 0 = blank) and your solution
// (\`priv.solution\`, 81 private cells). The guest checks the solution completes
// the puzzle and is a valid grid, and returns only a single valid/invalid bit.
// The solution itself is never revealed.

// Stage the public puzzle (known to both parties).
const puzzlePtr = await vm.callLocal("cabi_realloc", [
  Public(0), Public(0), Public(1), Public(81),
]);
vm.writePublic(puzzlePtr, pub.puzzle);

// Stage the private solution (81 cells).
const solPtr = await vm.callLocal("cabi_realloc", [
  Public(0), Public(0), Public(1), Public(81),
]);
vm.writePrivate(solPtr, priv.solution);

// Verify inside the VM; check returns the 1/0 verdict, revealed to both.
const ok = await vm.call("check", [Public(puzzlePtr), Public(solPtr)]);
return ok === 1 ? "valid solution ✓" : "invalid ✗";
`;

/// A 9×9 grid as 81 cells (row-major). Accepts digits 1-9, with 0, '.', or any
/// other character treated as a blank; ignores separators (spaces, newlines,
/// pipes). Always returns exactly 81 cells (padded/truncated) so the guest's
/// fixed-size buffers line up.
const parseGrid = (text: string): Uint8Array => {
  const cells = new Uint8Array(81);
  let n = 0;
  for (const ch of text) {
    if (n >= 81) break;
    if (ch >= "1" && ch <= "9") cells[n++] = ch.charCodeAt(0) - 48;
    else if (ch === "0" || ch === ".") cells[n++] = 0;
  }
  return cells;
};

export const EXAMPLES: ExampleProgram[] = [
  {
    id: "sha256",
    label: "sha-256",
    description:
      "Prove the SHA-256 digest of the provided message, hiding the input from the verifier.",
    moduleName: "sha256.wasm (built-in)",
    wasmUrl: "guests/sha256.wasm",
    script: SHA256_SCRIPT,
    fields: [
      {
        name: "message",
        label: "message to hash",
        value: "hello speakup",
        maxLength: 131072,
        presets: [
          { label: "1 KB", bytes: 1024 },
          { label: "4 KB", bytes: 4096 },
          { label: "16 KB", bytes: 16384 },
          { label: "64 KB", bytes: 65536 },
          { label: "128 KB", bytes: 131072 },
        ],
        presetsHint: "proving time scales with input size",
      },
    ],
    toInputs(values) {
      const message = new TextEncoder().encode(values.message ?? "");
      return { priv: { message }, pub: { len: message.length } };
    },
  },
  {
    id: "sudoku",
    label: "sudoku",
    description:
      "Prove the solution to a Sudoku puzzle without revealing it.",
    moduleName: "sudoku.wasm (built-in)",
    wasmUrl: "guests/sudoku.wasm",
    script: SUDOKU_SCRIPT,
    render: sudokuForm,
    toInputs(values) {
      return {
        priv: { solution: parseGrid(values.solution ?? "") },
        pub: { puzzle: parseGrid(values.puzzle ?? "") },
      };
    },
  },
];

/// Deterministic filler of an exact byte size for the preset buttons.
const filler = (bytes: number) =>
  "speakup demo data ".repeat(Math.ceil(bytes / 18)).slice(0, bytes);

const labelEl = (text: string) => {
  const el = document.createElement("div");
  el.className = "label";
  el.textContent = text;
  return el;
};

/// An example's tab button.
export function exampleTab(ex: ExampleProgram, active: boolean): HTMLButtonElement {
  const btn = document.createElement("button");
  btn.dataset.tab = ex.id;
  btn.textContent = ex.label;
  if (active) btn.classList.add("active");
  return btn;
}

export type MountedForm = FormBody;

/// The declarative field renderer: one labelled text/textarea input per field,
/// with optional size presets. Appends directly into `form` (preserving its
/// flex-gap spacing) and returns the values getter.
function fieldsBody(form: HTMLElement, fields: Field[]): () => Record<string, string> {
  const inputs = new Map<string, HTMLInputElement | HTMLTextAreaElement>();
  for (const field of fields) {
    form.append(labelEl(field.label));

    const input = field.rows && field.rows > 1
      ? document.createElement("textarea")
      : document.createElement("input");
    if (input instanceof HTMLTextAreaElement) {
      input.rows = field.rows!;
    } else {
      input.type = "text";
    }
    input.value = field.value;
    if (field.maxLength) input.maxLength = field.maxLength;
    inputs.set(field.name, input);
    form.append(input);

    if (field.presets?.length) {
      const row = document.createElement("div");
      row.className = "presets";
      const lead = document.createElement("span");
      lead.textContent = "presets:";
      row.append(lead);
      for (const preset of field.presets) {
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = preset.label;
        b.addEventListener("click", () => {
          input.value = filler(preset.bytes);
        });
        row.append(b);
      }
      if (field.presetsHint) {
        const hint = document.createElement("span");
        hint.className = "presets-hint";
        hint.textContent = field.presetsHint;
        row.append(hint);
      }
      form.append(row);
    }
  }
  return () => Object.fromEntries([...inputs].map(([k, i]) => [k, i.value]));
}

/// Build an example's form pane: a description box, the input region (a custom
/// `render` if given, else the declarative fields), and the "view script"
/// toggle (which carries the `js-view-script` class for the host to delegate).
export function exampleForm(ex: ExampleProgram): MountedForm {
  const form = document.createElement("div");
  form.className = "guest-form";
  form.dataset.example = ex.id;

  const desc = document.createElement("div");
  desc.className = "guest-desc";
  desc.append(labelEl("description"));
  const box = document.createElement("div");
  box.className = "guest-desc-box";
  const p = document.createElement("p");
  p.textContent = ex.description;
  box.append(p);
  desc.append(box);
  form.append(desc);

  let values: () => Record<string, string>;
  if (ex.render) {
    const body = ex.render();
    form.append(body.el);
    values = body.values;
  } else {
    values = fieldsBody(form, ex.fields ?? []);
  }

  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "link-btn js-view-script";
  toggle.textContent = "view script ▸";
  form.append(toggle);

  return { el: form, values };
}
