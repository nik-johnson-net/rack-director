import { useState, useRef } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Upload, Download, Trash2, Check, X } from "lucide-react";

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
  onDelete,
  accept,
}: FileUploadProps) {
  const [uploading, setUploading] = useState(false);
  const [deleting, setDeleting] = useState(false);
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

  const handleDelete = async () => {
    if (!onDelete) return;

    setDeleting(true);
    setError(null);

    try {
      await onDelete();
      setSuccess(true);
      setTimeout(() => setSuccess(false), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Delete failed");
    } finally {
      setDeleting(false);
    }
  };

  const hasFile = !!currentFile && currentFile.length > 0;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="font-medium">{label}:</span>
          {hasFile ? (
            <div className="flex items-center gap-1.5">
              <Check className="h-4 w-4 text-green-600 flex-shrink-0" />
              {filename ? (
                <Badge variant="secondary" className="font-mono text-xs">
                  {filename}
                </Badge>
              ) : (
                <span className="text-green-600 text-sm">Uploaded</span>
              )}
            </div>
          ) : (
            <span className="flex items-center gap-1 text-gray-400 text-sm">
              <X className="h-4 w-4" />
              Not uploaded
            </span>
          )}
        </div>

        <div className="flex gap-2">
          {hasFile && onDownload && (
            <Button
              variant="outline"
              size="sm"
              onClick={onDownload}
            >
              <Download className="h-4 w-4 mr-1" />
              Download
            </Button>
          )}
          {hasFile && onDelete && (
            <Button
              variant="outline"
              size="sm"
              onClick={handleDelete}
              disabled={deleting}
            >
              <Trash2 className="h-4 w-4 mr-1" />
              {deleting ? "Deleting..." : "Delete"}
            </Button>
          )}
          <Button
            variant={hasFile ? "outline" : "default"}
            size="sm"
            onClick={() => fileInputRef.current?.click()}
            disabled={uploading}
          >
            <Upload className="h-4 w-4 mr-1" />
            {uploading ? "Uploading..." : hasFile ? "Replace" : "Upload"}
          </Button>
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
        <div className="bg-red-50 border border-red-200 text-red-800 px-3 py-2 rounded text-sm">
          {error}
        </div>
      )}

      {success && (
        <div className="bg-green-50 border border-green-200 text-green-800 px-3 py-2 rounded text-sm">
          Operation successful!
        </div>
      )}
    </div>
  );
}
