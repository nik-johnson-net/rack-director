import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { FormFieldError } from "@/components/ui/form-field-error";
import { cn } from "@/lib/utils";

interface FormFieldProps {
  id: string;
  label: string;
  required?: boolean;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  helperText?: string;
  error?: string;
  onClearError?: () => void;
  type?: string;
  disabled?: boolean;
  className?: string;
  inputClassName?: string;
  children?: React.ReactNode; // For custom input rendering (Select, Textarea, etc.)
}

export function FormField({
  id,
  label,
  required = false,
  value,
  onChange,
  placeholder,
  helperText,
  error,
  onClearError,
  type = "text",
  disabled = false,
  className,
  inputClassName,
  children,
}: FormFieldProps) {
  return (
    <div className={cn("space-y-2", className)}>
      <Label htmlFor={id}>
        {label} {required && "*"}
      </Label>
      {children ? (
        children
      ) : (
        <Input
          id={id}
          type={type}
          value={value}
          onChange={(e) => {
            onChange(e.target.value);
            onClearError?.();
          }}
          placeholder={placeholder}
          required={required}
          disabled={disabled}
          aria-invalid={!!error}
          className={inputClassName}
        />
      )}
      {helperText && !error && (
        <p className="text-xs text-muted-foreground">{helperText}</p>
      )}
      <FormFieldError error={error} />
    </div>
  );
}

interface FormTextareaFieldProps extends Omit<FormFieldProps, 'type' | 'children'> {
  rows?: number;
}

export function FormTextareaField({
  id,
  label,
  required = false,
  value,
  onChange,
  placeholder,
  helperText,
  error,
  onClearError,
  disabled = false,
  className,
  inputClassName,
  rows = 3,
}: FormTextareaFieldProps) {
  return (
    <div className={cn("space-y-2", className)}>
      <Label htmlFor={id}>
        {label} {required && "*"}
      </Label>
      <Textarea
        id={id}
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          onClearError?.();
        }}
        placeholder={placeholder}
        required={required}
        disabled={disabled}
        aria-invalid={!!error}
        rows={rows}
        className={inputClassName}
      />
      {helperText && !error && (
        <p className="text-xs text-muted-foreground">{helperText}</p>
      )}
      <FormFieldError error={error} />
    </div>
  );
}

interface FormSelectFieldProps {
  id: string;
  label: string;
  required?: boolean;
  value: string | number;
  onChange: (value: string) => void;
  options: { value: string | number; label: string }[];
  placeholder?: string;
  helperText?: string;
  error?: string;
  onClearError?: () => void;
  disabled?: boolean;
  className?: string;
}

export function FormSelectField({
  id,
  label,
  required = false,
  value,
  onChange,
  options,
  placeholder,
  helperText,
  error,
  onClearError,
  disabled = false,
  className,
}: FormSelectFieldProps) {
  return (
    <div className={cn("space-y-2", className)}>
      <Label htmlFor={id}>
        {label} {required && "*"}
      </Label>
      <select
        id={id}
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          onClearError?.();
        }}
        className="border-input placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:bg-input/30 flex h-10 w-full rounded-md border bg-transparent px-3 py-2 text-base shadow-xs transition-[color,box-shadow] outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50 md:text-sm"
        required={required}
        disabled={disabled}
        aria-invalid={!!error}
      >
        {placeholder && <option value="">{placeholder}</option>}
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
      {helperText && !error && (
        <p className="text-xs text-muted-foreground">{helperText}</p>
      )}
      <FormFieldError error={error} />
    </div>
  );
}
