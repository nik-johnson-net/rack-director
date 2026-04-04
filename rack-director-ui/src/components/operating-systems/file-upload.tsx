import { useState, useRef } from "react";
import { Upload, Download, Check, X } from "lucide-react";

interface FileUploadProps {
  label: string;
  currentFile?: string;
  filename?: string;
  onUpload: (file: File) => Promise<void>;
  onDownload?: () => void;
  onDelete?: () => Promise<void>;
  accept?: string;
}

export default function FileUpload({
  label,
  currentFile,
  filename,
  onUpload,
  onDownload,
  accept,
}: FileUploadProps) {
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setUploading(true);
    setError(null);
    setSuccess(false);

    try {
      await onUpload(file);
      setSuccess(true);
      setTimeout(() => setSuccess(false), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Upload failed");
    } finally {
      setUploading(false);
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
    }
  };

  const hasFile = !!currentFile && currentFile.length > 0;

  return (
    <div>
      <div className="flex items-center justify-between">
        {/* Label + status */}
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold text-text-secondary uppercase tracking-[0.5px]">
            {label}
          </span>
          {hasFile ? (
            <span className="flex items-center gap-1 text-xs text-status-provisioned">
              <Check className="h-3.5 w-3.5" />
              {filename ? (
                <span className="font-mono text-text-secondary">{filename}</span>
              ) : (
                "Uploaded"
              )}
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs text-text-muted">
              <X className="h-3.5 w-3.5" />
              Not uploaded
            </span>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-3">
          {hasFile && onDownload && (
            <button
              onClick={onDownload}
              className="flex items-center gap-1 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
            >
              <Download className="h-3.5 w-3.5" />
              download
            </button>
          )}
          <button
            onClick={() => fileInputRef.current?.click()}
            disabled={uploading}
            className="flex items-center gap-1 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Upload className="h-3.5 w-3.5" />
            {uploading ? "uploading..." : hasFile ? "replace" : "upload"}
          </button>
          <input
            ref={fileInputRef}
            type="file"
            onChange={handleFileChange}
            accept={accept}
            className="hidden"
          />
        </div>
      </div>

      {error && (
        <div className="mt-1 bg-error-bg border border-error-border text-status-broken px-3 py-1.5 text-xs">
          {error}
        </div>
      )}

      {success && (
        <div className="mt-1 bg-status-provisioned-bg border border-status-provisioned/30 text-status-provisioned px-3 py-1.5 text-xs">
          Upload successful
        </div>
      )}
    </div>
  );
}
