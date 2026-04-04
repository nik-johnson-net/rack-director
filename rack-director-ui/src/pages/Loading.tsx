import { Loader2 } from "lucide-react";

function Loading() {
  return (
    <div className="flex items-center justify-center h-full min-h-[200px]">
      <div className="flex items-center gap-3 text-text-secondary">
        <Loader2 className="h-5 w-5 animate-spin text-accent" />
        <span className="text-sm">Loading...</span>
      </div>
    </div>
  );
}

export default Loading;
