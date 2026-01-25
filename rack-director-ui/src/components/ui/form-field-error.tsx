import { AlertCircle } from "lucide-react";

interface FormFieldErrorProps {
  error?: string;
}

export function FormFieldError({ error }: FormFieldErrorProps) {
  if (!error) return null;

  return (
    <div className="flex items-start gap-2 text-sm text-red-600">
      <AlertCircle className="h-4 w-4 mt-0.5 flex-shrink-0" />
      <span>{error}</span>
    </div>
  );
}
