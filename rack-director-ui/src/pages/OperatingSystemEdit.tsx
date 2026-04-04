import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PageHeader } from "@/components/ui/page-header";
import { DeleteConfirmationDialog } from "@/components/ui/delete-confirmation-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import FileUpload from "@/components/operating-systems/file-upload";
import TemplateDocs from "@/components/operating-systems/template-docs";
import {
  type OperatingSystemWithArchitectures,
  type Architecture,
  updateOperatingSystem,
  deleteOperatingSystem,
  createOsArchitecture,
  deleteOsArchitecture,
  uploadKernel,
  uploadInitramfs,
  uploadModule,
  uploadInstallScript,
  getDownloadUrl,
  getOperatingSystem,
} from "@/lib/client";
import { Trash2, Plus, ChevronDown, ChevronRight } from "lucide-react";

const labelCls =
  "block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1";

const inputCls =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted";

const selectCls =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded focus:outline-none focus:border-accent appearance-none";

function OperatingSystemEdit() {
  const initialData = useLoaderData<OperatingSystemWithArchitectures>();
  const navigate = useNavigate();
  const params = useParams<{ id: string }>();
  const osId = parseInt(params.id!);

  const [data, setData] = useState(initialData);
  const [editingBasic, setEditingBasic] = useState(false);
  const [name, setName] = useState(data.name);
  const [version, setVersion] = useState(data.version);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedArchs, setExpandedArchs] = useState<Record<string, boolean>>({});
  const [addArchDialogOpen, setAddArchDialogOpen] = useState(false);
  const [newArchitecture, setNewArchitecture] = useState<Architecture>("x86-64");
  const [moduleDialogOpen, setModuleDialogOpen] = useState<string | null>(null);
  const [moduleName, setModuleName] = useState("");
  const [cmdlineArgs, setCmdlineArgs] = useState<Record<string, string>>({});
  const [savingCmdline, setSavingCmdline] = useState<string | null>(null);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleteArchDialogOpen, setDeleteArchDialogOpen] = useState(false);
  const [archToDelete, setArchToDelete] = useState<Architecture | null>(null);

  useEffect(() => {
    const expanded: Record<string, boolean> = {};
    const cmdline: Record<string, string> = {};
    data.architectures.forEach((arch) => {
      expanded[arch.architecture] = true;
      cmdline[arch.architecture] = arch.cmdline_args || "";
    });
    setExpandedArchs(expanded);
    setCmdlineArgs(cmdline);
  }, [data.architectures]);

  const refreshData = async () => {
    const updated = await getOperatingSystem(osId);
    setData(updated);
  };

  const handleUpdateBasic = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsSubmitting(true);
    try {
      const updated = await updateOperatingSystem(osId, { name, version });
      setData({ ...data, ...updated });
      setEditingBasic(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update operating system");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDelete = async () => {
    await deleteOperatingSystem(osId);
    navigate("/operating-systems");
  };

  const handleAddArchitecture = async () => {
    try {
      await createOsArchitecture(osId, {
        architecture: newArchitecture,
        kernel_path: "",
        initramfs_path: "",
        modules: [],
      });
      await refreshData();
      setAddArchDialogOpen(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to add architecture");
    }
  };

  const openDeleteArchDialog = (arch: Architecture) => {
    setArchToDelete(arch);
    setDeleteArchDialogOpen(true);
  };

  const handleDeleteArchitecture = async () => {
    if (!archToDelete) return;
    await deleteOsArchitecture(osId, archToDelete);
    await refreshData();
    setArchToDelete(null);
  };

  const handleUploadModule = async (arch: Architecture, file: File) => {
    if (!moduleName.trim()) {
      throw new Error("Module name is required");
    }
    await uploadModule(osId, arch, file, moduleName);
    await refreshData();
    setModuleName("");
    setModuleDialogOpen(null);
  };

  const toggleArchExpanded = (arch: string) => {
    setExpandedArchs((prev) => ({ ...prev, [arch]: !prev[arch] }));
  };

  const handleSaveCmdlineArgs = async (arch: Architecture) => {
    setSavingCmdline(arch);
    setError(null);
    try {
      const archData = data.architectures.find((a) => a.architecture === arch);
      if (!archData) return;
      await createOsArchitecture(osId, {
        architecture: arch,
        kernel_path: archData.kernel_path,
        initramfs_path: archData.initramfs_path,
        modules: archData.modules,
        cmdline_args: cmdlineArgs[arch] || undefined,
        install_script_path: archData.install_script_path || undefined,
      });
      await refreshData();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save kernel arguments");
    } finally {
      setSavingCmdline(null);
    }
  };

  return (
    <div className="max-w-4xl">
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "OS Images", href: "/operating-systems" },
          { label: `${data.name} ${data.version}` },
        ]}
        title={`${data.name} ${data.version}`}
        actions={
          <Button variant="destructive" onClick={() => setDeleteDialogOpen(true)}>
            <Trash2 className="h-4 w-4 mr-2" />
            Delete
          </Button>
        }
      />

      {error && (
        <div className="bg-error-bg border border-error-border text-status-broken px-3 py-2 text-xs mb-4">
          {error}
        </div>
      )}

      {/* Basic Information */}
      <div className="bg-bg-surface border border-border p-4 mb-4">
        <div className="flex items-center justify-between mb-3">
          <p className="text-xs font-semibold text-text-primary">Basic Information</p>
          {!editingBasic && (
            <button
              onClick={() => setEditingBasic(true)}
              className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
            >
              edit
            </button>
          )}
        </div>

        {editingBasic ? (
          <form onSubmit={handleUpdateBasic}>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 mb-4">
              <div>
                <label htmlFor="edit-name" className={labelCls}>
                  Name <span className="text-accent">*</span>
                </label>
                <Input
                  id="edit-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  required
                  className={inputCls}
                />
              </div>
              <div>
                <label htmlFor="edit-version" className={labelCls}>
                  Version <span className="text-accent">*</span>
                </label>
                <Input
                  id="edit-version"
                  value={version}
                  onChange={(e) => setVersion(e.target.value)}
                  required
                  className={inputCls}
                />
              </div>
            </div>
            <div className="flex gap-2">
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? "Saving..." : "Save"}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  setEditingBasic(false);
                  setName(data.name);
                  setVersion(data.version);
                }}
              >
                Cancel
              </Button>
            </div>
          </form>
        ) : (
          <div className="grid grid-cols-[140px_1fr] gap-x-4 gap-y-1">
            <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">Name</span>
            <span className="text-xs text-text-primary">{data.name}</span>
            <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">Version</span>
            <span className="text-xs text-text-primary">{data.version}</span>
            {data.description && (
              <>
                <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">Description</span>
                <span className="text-xs text-text-primary">{data.description}</span>
              </>
            )}
          </div>
        )}
      </div>

      {/* Architectures */}
      <div className="bg-bg-surface border border-border">
        <div className="flex items-center justify-between px-4 py-3 border-b border-border">
          <p className="text-xs font-semibold text-text-primary">Architectures</p>

          <Dialog open={addArchDialogOpen} onOpenChange={setAddArchDialogOpen}>
            <DialogTrigger asChild>
              <button className="flex items-center gap-1 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer">
                <Plus className="h-3 w-3" />
                Add Architecture
              </button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Add Architecture</DialogTitle>
                <DialogDescription>Add support for a new architecture</DialogDescription>
              </DialogHeader>
              <div className="space-y-3">
                <div>
                  <label htmlFor="arch-select" className={labelCls}>
                    Architecture
                  </label>
                  <select
                    id="arch-select"
                    value={newArchitecture}
                    onChange={(e) => setNewArchitecture(e.target.value as Architecture)}
                    className={selectCls}
                  >
                    <option value="x86-64">x86-64</option>
                  </select>
                </div>
              </div>
              <DialogFooter>
                <Button onClick={handleAddArchitecture}>Add</Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </div>

        {data.architectures.length === 0 ? (
          <div className="px-4 py-8 text-center text-xs text-text-muted">
            No architectures configured. Add one to get started.
          </div>
        ) : (
          <div className="divide-y divide-border">
            {data.architectures.map((archData) => (
              <div key={archData.architecture}>
                {/* Architecture header row */}
                <div className="flex items-center justify-between px-4 py-3 bg-bg-raised">
                  <button
                    onClick={() => toggleArchExpanded(archData.architecture)}
                    className="flex items-center gap-2 text-xs font-semibold text-text-primary hover:text-accent transition-colors cursor-pointer"
                    aria-label={`Toggle ${archData.architecture} section`}
                  >
                    {expandedArchs[archData.architecture] ? (
                      <ChevronDown className="h-3.5 w-3.5 text-text-muted" />
                    ) : (
                      <ChevronRight className="h-3.5 w-3.5 text-text-muted" />
                    )}
                    <span className="inline-flex items-center px-2 py-0.5 rounded-sm text-xs font-medium bg-accent-muted text-accent border border-accent/20">
                      {archData.architecture}
                    </span>
                  </button>
                  <button
                    onClick={() => openDeleteArchDialog(archData.architecture)}
                    className="text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                    aria-label={`Delete architecture ${archData.architecture}`}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>

                {expandedArchs[archData.architecture] && (
                  <div className="p-4 space-y-6">
                    {/* Kernel */}
                    <FileUpload
                      label="Kernel"
                      currentFile={archData.kernel_path}
                      filename={archData.kernel_filename}
                      onUpload={async (file) => {
                        await uploadKernel(osId, archData.architecture, file);
                        await refreshData();
                      }}
                      onDownload={() => {
                        window.location.href = getDownloadUrl(osId, archData.architecture, "kernel");
                      }}
                    />

                    {/* Initramfs */}
                    <FileUpload
                      label="Initramfs"
                      currentFile={archData.initramfs_path}
                      filename={archData.initramfs_filename}
                      onUpload={async (file) => {
                        await uploadInitramfs(osId, archData.architecture, file);
                        await refreshData();
                      }}
                      onDownload={() => {
                        window.location.href = getDownloadUrl(osId, archData.architecture, "initramfs");
                      }}
                    />

                    {/* Modules */}
                    <div>
                      <p className="text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-2">
                        Modules
                      </p>
                      {archData.modules.length === 0 ? (
                        <p className="text-xs text-text-muted mb-2">No modules uploaded</p>
                      ) : (
                        <div className="space-y-1 mb-2">
                          {archData.modules.map((module) => (
                            <div
                              key={module}
                              className="flex items-center justify-between bg-bg-base border border-border px-3 py-2"
                            >
                              <span className="text-xs text-text-primary font-mono">{module}</span>
                              <button
                                onClick={() => {
                                  window.location.href = getDownloadUrl(
                                    osId,
                                    archData.architecture,
                                    `modules/${module}`
                                  );
                                }}
                                className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                              >
                                download
                              </button>
                            </div>
                          ))}
                        </div>
                      )}

                      <Dialog
                        open={moduleDialogOpen === archData.architecture}
                        onOpenChange={(open) =>
                          setModuleDialogOpen(open ? archData.architecture : null)
                        }
                      >
                        <DialogTrigger asChild>
                          <button className="flex items-center gap-1 text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer">
                            <Plus className="h-3 w-3" />
                            Upload Module
                          </button>
                        </DialogTrigger>
                        <DialogContent>
                          <DialogHeader>
                            <DialogTitle>Upload Module</DialogTitle>
                            <DialogDescription>
                              Enter a name for the module and select the file to upload
                            </DialogDescription>
                          </DialogHeader>
                          <div className="space-y-4">
                            <div>
                              <label htmlFor="module-name" className={labelCls}>
                                Module Name
                              </label>
                              <Input
                                id="module-name"
                                value={moduleName}
                                onChange={(e) => setModuleName(e.target.value)}
                                placeholder="e.g., network-driver.ko"
                                className={inputCls}
                              />
                            </div>
                            <FileUpload
                              label="Module File"
                              currentFile={undefined}
                              onUpload={(file) =>
                                handleUploadModule(archData.architecture, file)
                              }
                            />
                          </div>
                        </DialogContent>
                      </Dialog>
                    </div>

                    {/* Install Script */}
                    <div className="space-y-3">
                      <FileUpload
                        label="Install Script"
                        currentFile={archData.install_script_path}
                        filename={archData.install_script_filename}
                        onUpload={async (file) => {
                          await uploadInstallScript(osId, archData.architecture, file);
                          await refreshData();
                        }}
                        onDownload={() => {
                          window.location.href = getDownloadUrl(
                            osId,
                            archData.architecture,
                            "install_script"
                          );
                        }}
                      />
                      <TemplateDocs type="install-script" />
                    </div>

                    {/* Kernel Cmdline Args */}
                    <div className="space-y-3">
                      <div>
                        <label
                          htmlFor={`cmdline-${archData.architecture}`}
                          className={labelCls}
                        >
                          Kernel Command Line Arguments
                        </label>
                        <textarea
                          id={`cmdline-${archData.architecture}`}
                          value={cmdlineArgs[archData.architecture] || ""}
                          onChange={(e) =>
                            setCmdlineArgs((prev) => ({
                              ...prev,
                              [archData.architecture]: e.target.value,
                            }))
                          }
                          placeholder="Additional kernel boot parameters"
                          rows={2}
                          className="w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded resize-none focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted font-mono"
                        />
                        <Button
                          size="sm"
                          className="mt-2"
                          onClick={() => handleSaveCmdlineArgs(archData.architecture)}
                          disabled={savingCmdline === archData.architecture}
                        >
                          {savingCmdline === archData.architecture ? "Saving..." : "Save"}
                        </Button>
                      </div>
                      <TemplateDocs type="cmdline" />
                    </div>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      <DeleteConfirmationDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        title="Delete Operating System?"
        description="This will permanently delete this operating system. This action cannot be undone. Any roles using this OS will need to be updated."
        onConfirm={handleDelete}
      />

      <DeleteConfirmationDialog
        open={deleteArchDialogOpen}
        onOpenChange={setDeleteArchDialogOpen}
        title="Delete Architecture?"
        description="This will remove all uploaded files for this architecture. This cannot be undone."
        itemName={archToDelete ?? undefined}
        onConfirm={handleDeleteArchitecture}
      />
    </div>
  );
}

export default OperatingSystemEdit;
