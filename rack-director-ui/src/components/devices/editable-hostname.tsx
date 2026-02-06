import { useState, useEffect, useRef } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pencil, Loader2 } from "lucide-react";
import { updateDeviceAttributes } from "@/lib/client";
import { ValidationError } from "@/lib/client";

interface EditableHostnameProps {
  uuid: string;
  hostname: string;
  onHostnameChange?: (newHostname: string) => void;
  onError?: (error: string) => void;
}

export function EditableHostname({
  uuid,
  hostname,
  onHostnameChange,
  onError,
}: EditableHostnameProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [value, setValue] = useState(hostname);
  const [isSaving, setIsSaving] = useState(false);
  const [validationError, setValidationError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Sync with prop changes
  useEffect(() => {
    setValue(hostname);
  }, [hostname]);

  // Focus input when editing starts
  useEffect(() => {
    if (isEditing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [isEditing]);

  const handleSave = async () => {
    // Don't save if value hasn't changed
    if (value === hostname) {
      setIsEditing(false);
      setValidationError(null);
      return;
    }

    setIsSaving(true);
    setValidationError(null);

    try {
      // Update only the hostname attribute
      await updateDeviceAttributes(uuid, { hostname: value });

      // Notify parent component of successful change
      if (onHostnameChange) {
        onHostnameChange(value);
      }

      setIsEditing(false);
    } catch (err) {
      // Handle validation errors from backend
      if (err instanceof ValidationError) {
        const hostnameError = err.errors.hostname;
        if (hostnameError) {
          setValidationError(hostnameError);
        } else {
          setValidationError("Validation failed");
        }
      } else {
        const errorMessage = err instanceof Error ? err.message : "Failed to update hostname";
        if (onError) {
          onError(errorMessage);
        }
      }
      // Reset value to original on error
      setValue(hostname);
    } finally {
      setIsSaving(false);
    }
  };

  const handleCancel = () => {
    setValue(hostname);
    setIsEditing(false);
    setValidationError(null);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleSave();
    } else if (e.key === "Escape") {
      e.preventDefault();
      handleCancel();
    }
  };

  if (isEditing) {
    return (
      <div className="space-y-2">
        <div className="flex items-center gap-2">
          <Input
            ref={inputRef}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={handleKeyDown}
            onBlur={handleSave}
            className="h-9 max-w-xs font-medium text-lg"
            disabled={isSaving}
            aria-invalid={!!validationError}
            aria-label="Edit hostname"
          />
          {isSaving && <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />}
        </div>
        {validationError && (
          <p className="text-sm text-destructive">{validationError}</p>
        )}
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2 group">
      <span className="text-3xl font-bold tracking-tight">{hostname}</span>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity"
        onClick={() => setIsEditing(true)}
        aria-label="Edit hostname"
      >
        <Pencil className="h-4 w-4" />
      </Button>
    </div>
  );
}
