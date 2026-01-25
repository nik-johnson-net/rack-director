import { useState, useCallback } from "react";

export type FieldErrors = Record<string, string>;

export function useFieldErrors() {
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});

  const clearAllErrors = useCallback(() => {
    setFieldErrors({});
  }, []);

  const clearFieldError = useCallback((field: string) => {
    setFieldErrors(prev => {
      const next = { ...prev };
      delete next[field];
      return next;
    });
  }, []);

  const setErrors = useCallback((errors: FieldErrors) => {
    setFieldErrors(errors);
  }, []);

  const hasError = useCallback((field: string) => {
    return field in fieldErrors;
  }, [fieldErrors]);

  const getError = useCallback((field: string) => {
    return fieldErrors[field];
  }, [fieldErrors]);

  return {
    fieldErrors,
    clearAllErrors,
    clearFieldError,
    setErrors,
    hasError,
    getError,
  };
}
