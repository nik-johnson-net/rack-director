import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { OsmModule, OsmOperatingSystem, OsmTemplateVariable } from "@/lib/client";

export function findOsByKey(
  key: string,
  osList: OsmOperatingSystem[],
  modules: OsmModule[]
): OsmOperatingSystem | undefined {
  const [mod, name, release] = key.split("|");
  return osList.find(
    (os) =>
      os.name === name &&
      os.release === release &&
      modules.find((m) => m.id === os.module_id)?.name === mod
  );
}

function defaultForType(type: OsmTemplateVariable["type"]): unknown {
  switch (type) {
    case "string": return "";
    case "integer": return "";
    case "boolean": return false;
    case "list": return [];
  }
}

export function buildDefaultValues(vars: OsmTemplateVariable[]): Record<string, unknown> {
  return mergeOsValues({}, vars);
}

export function mergeOsValues(
  current: Record<string, unknown>,
  vars: OsmTemplateVariable[]
): Record<string, unknown> {
  const next: Record<string, unknown> = {};
  for (const v of vars) {
    if (v.name in current) {
      next[v.name] = current[v.name];
    } else if (v.default !== null && v.default !== undefined) {
      next[v.name] = v.type === "integer" ? String(v.default) : v.default;
    } else {
      next[v.name] = defaultForType(v.type);
    }
  }
  return next;
}

export function buildSubmitConfig(
  values: Record<string, unknown>,
  vars: OsmTemplateVariable[]
): Record<string, unknown> | undefined {
  const result: Record<string, unknown> = {};
  for (const v of vars) {
    const val = values[v.name];
    if (v.type === "integer") {
      const n = parseInt(val as string, 10);
      if (!isNaN(n)) result[v.name] = n;
    } else if (v.type === "list") {
      const filtered = ((val as string[]) ?? []).filter((s) => s.trim());
      if (filtered.length > 0) result[v.name] = filtered;
    } else if (v.type === "boolean") {
      result[v.name] = (val as boolean) ?? false;
    } else {
      if (val !== "" && val !== undefined && val !== null) result[v.name] = val;
    }
  }
  return Object.keys(result).length > 0 ? result : undefined;
}

export function validateRequiredVars(
  vars: OsmTemplateVariable[],
  values: Record<string, unknown>
): string[] {
  return vars
    .filter((v) => {
      if (!v.required) return false;
      const val = values[v.name];
      if (v.type === "boolean") return false;
      if (v.type === "list") {
        const list = (val as string[]) ?? [];
        return list.length === 0 || list.every((s) => !s.trim());
      }
      return val === "" || val === undefined || val === null;
    })
    .map((v) => v.name);
}

interface OsVariableFormProps {
  variables: OsmTemplateVariable[];
  values: Record<string, unknown>;
  onChange: (values: Record<string, unknown>) => void;
}

export function OsVariableForm({ variables, values, onChange }: OsVariableFormProps) {
  if (variables.length === 0) {
    return (
      <p className="text-xs text-text-muted italic">
        No configurable variables for this operating system.
      </p>
    );
  }

  const set = (key: string, value: unknown) => {
    onChange({ ...values, [key]: value });
  };

  const setListItem = (key: string, idx: number, val: string) => {
    const list = (values[key] as string[]) ?? [];
    const next = [...list];
    next[idx] = val;
    set(key, next);
  };

  const removeListItem = (key: string, idx: number) => {
    const list = (values[key] as string[]) ?? [];
    set(key, list.filter((_, i) => i !== idx));
  };

  const addListItem = (key: string) => {
    const list = (values[key] as string[]) ?? [];
    set(key, [...list, ""]);
  };

  return (
    <div className="space-y-4">
      {variables.map((v) => (
        <div key={v.name} className="space-y-1">
          <Label className="text-xs text-text-secondary uppercase tracking-[0.5px]">
            {v.name}
            {v.required && <span className="text-status-broken"> *</span>}
          </Label>
          {v.description && (
            <p className="text-xs text-text-muted">{v.description}</p>
          )}

          {v.type === "boolean" ? (
            <div className="flex items-center gap-2">
              <input
                id={`osvar_${v.name}`}
                type="checkbox"
                checked={(values[v.name] as boolean) ?? false}
                onChange={(e) => set(v.name, e.target.checked)}
                className="accent-accent"
              />
              <label
                htmlFor={`osvar_${v.name}`}
                className="text-xs text-text-secondary cursor-pointer"
              >
                {(values[v.name] as boolean) ? "Enabled" : "Disabled"}
              </label>
            </div>
          ) : v.type === "list" ? (
            <div className="space-y-1">
              {((values[v.name] as string[]) ?? []).map((item, idx) => (
                <div key={idx} className="flex gap-2 items-center">
                  <Input
                    value={item}
                    onChange={(e) => setListItem(v.name, idx, e.target.value)}
                    className="h-8 text-xs flex-1"
                  />
                  <button
                    type="button"
                    onClick={() => removeListItem(v.name, idx)}
                    className="text-text-muted hover:text-status-broken transition-colors text-sm leading-none px-1 shrink-0"
                    aria-label="Remove item"
                  >
                    ×
                  </button>
                </div>
              ))}
              <button
                type="button"
                onClick={() => addListItem(v.name)}
                className="text-xs text-text-secondary border border-border px-2 py-1 hover:border-accent hover:text-text-primary transition-colors rounded-sm"
              >
                + Add item
              </button>
            </div>
          ) : v.type === "integer" ? (
            <Input
              id={`osvar_${v.name}`}
              type="number"
              value={(values[v.name] as string) ?? ""}
              onChange={(e) => set(v.name, e.target.value)}
              className="h-8 text-xs"
            />
          ) : (
            <Input
              id={`osvar_${v.name}`}
              type="text"
              value={(values[v.name] as string) ?? ""}
              onChange={(e) => set(v.name, e.target.value)}
              className="h-8 text-xs"
            />
          )}
        </div>
      ))}
    </div>
  );
}
