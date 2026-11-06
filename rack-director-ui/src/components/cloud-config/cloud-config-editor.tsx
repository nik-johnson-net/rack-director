import { FormFieldError } from "@/components/ui/form-field-error";

/**
 * Standard snippet that installs root SSH keys through cloud-config. `lock_passwd: false`
 * keeps the installer's root password working: cloud-init locks the password of every listed
 * user by default, existing ones included.
 */
const ROOT_SSH_KEY_SNIPPET = `disable_root: false
users:
  - name: root
    lock_passwd: false
    ssh_authorized_keys:
      - ssh-ed25519 AAAA... admin@example.com
`;

interface CloudConfigEditorProps {
  id: string;
  value: string;
  onChange: (value: string) => void;
  error?: string;
  disabled?: boolean;
  placeholder?: string;
}

/**
 * Monospace editor for a cloud-config layer (global or role).
 *
 * Plain `#cloud-config` YAML only; rack-director validates it on save and merges the global
 * layer, then the role layer, when a device first boots.
 */
export function CloudConfigEditor({
  id,
  value,
  onChange,
  error,
  disabled,
  placeholder = "#cloud-config\npackages:\n  - jq",
}: CloudConfigEditorProps) {
  const insertExample = () => {
    const existing = value.replace(/\s+$/, "");
    onChange(
      existing
        ? `${existing}\n${ROOT_SSH_KEY_SNIPPET}`
        : `#cloud-config\n${ROOT_SSH_KEY_SNIPPET}`
    );
  };

  return (
    <div className="space-y-1">
      <textarea
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
        placeholder={placeholder}
        spellCheck={false}
        aria-invalid={!!error}
        rows={12}
        className="w-full min-h-48 resize-y bg-bg-base border border-border rounded-sm px-3 py-2 font-mono text-xs text-text-primary placeholder:text-text-muted outline-none transition-colors focus:border-accent focus:ring-1 focus:ring-accent/50 aria-invalid:border-status-broken aria-invalid:ring-1 aria-invalid:ring-status-broken/30 disabled:opacity-50 disabled:cursor-not-allowed"
      />
      <FormFieldError error={error} />
      <div className="flex items-start justify-between gap-4">
        <p className="text-xs text-text-muted">
          Plain <code>#cloud-config</code> YAML. Merged global first, then role: maps merge,
          lists append, and the role wins on single values.
        </p>
        <button
          type="button"
          onClick={insertExample}
          disabled={disabled}
          className="shrink-0 text-xs text-text-secondary border border-border px-2 py-1 hover:border-accent hover:text-text-primary transition-colors rounded-sm disabled:opacity-50 disabled:cursor-not-allowed"
        >
          + Root SSH key example
        </button>
      </div>
    </div>
  );
}
