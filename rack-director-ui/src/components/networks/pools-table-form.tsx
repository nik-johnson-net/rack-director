import type { CreateDhcpPoolRequest } from "@/lib/client";

interface PoolsTableFormProps {
  formData: CreateDhcpPoolRequest;
  onSubmit: (e: React.FormEvent) => void;
  setFormData: (data: CreateDhcpPoolRequest) => void;
  isSubmitting: boolean;
  error: string | null;
  editingPool: boolean;
}

const inputCls =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted";

const labelCls =
  "block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1";

export default function PoolsTableForm({
  formData,
  onSubmit,
  setFormData,
  isSubmitting,
  error,
  editingPool,
}: PoolsTableFormProps) {
  return (
    <form onSubmit={onSubmit} className="space-y-4">
      {error && (
        <div className="px-3 py-2 bg-error-bg border-l-[3px] border-status-broken text-xs text-status-broken">
          {error}
        </div>
      )}

      <div>
        <label htmlFor="pool-name" className={labelCls}>
          Pool Name <span className="text-status-broken">*</span>
        </label>
        <input
          id="pool-name"
          type="text"
          value={formData.name}
          onChange={(e) => setFormData({ ...formData, name: e.target.value })}
          placeholder="e.g., Main Pool"
          required
          className={inputCls}
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div>
          <label htmlFor="range-start" className={labelCls}>
            Range Start <span className="text-status-broken">*</span>
          </label>
          <input
            id="range-start"
            type="text"
            value={formData.range_start}
            onChange={(e) => setFormData({ ...formData, range_start: e.target.value })}
            placeholder="e.g., 192.168.1.100"
            required
            className={inputCls}
          />
        </div>

        <div>
          <label htmlFor="range-end" className={labelCls}>
            Range End <span className="text-status-broken">*</span>
          </label>
          <input
            id="range-end"
            type="text"
            value={formData.range_end}
            onChange={(e) => setFormData({ ...formData, range_end: e.target.value })}
            placeholder="e.g., 192.168.1.200"
            required
            className={inputCls}
          />
        </div>
      </div>

      <div className="flex justify-end gap-2 pt-2 border-t border-border">
        <button
          type="submit"
          disabled={isSubmitting}
          className="px-4 py-2 h-8 text-xs font-medium bg-accent text-bg-base border border-accent rounded hover:bg-accent-hover disabled:opacity-50 disabled:pointer-events-none cursor-pointer transition-colors"
        >
          {isSubmitting ? "Saving..." : editingPool ? "Update Pool" : "Add Pool"}
        </button>
      </div>
    </form>
  );
}
