import { useState, useEffect } from "react";
import { useLoaderData, useNavigate, useParams } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { PageHeader } from "@/components/ui/page-header";
import { FormField, FormTextareaField } from "@/components/ui/form-field";
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

function OperatingSystemEdit() {
  const initialData = useLoaderData<OperatingSystemWithArchitectures>();
  const navigate = useNavigate();
  const params = useParams<{ id: string }>();
  const osId = parseInt(params.id!);

  const [data, setData] = useState(initialData);
  const [editingBasic, setEditingBasic] = useState(false);
  const [name, setName] = useState(data.name);
  const [version, setVersion] = useState(data.version);
  const [description, setDescription] = useState(data.description || "");
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
    // Expand all architectures by default and initialize cmdline args
    const expanded: Record<string, boolean> = {};
    const cmdline: Record<string, string> = {};
    data.architectures.forEach(arch => {
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
      const updated = await updateOperatingSystem(osId, {
        name,
        version,
        description: description || undefined,
      });
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
    navigate('/operating-systems');
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
    setExpandedArchs(prev => ({ ...prev, [arch]: !prev[arch] }));
  };

  const handleSaveCmdlineArgs = async (arch: Architecture) => {
    setSavingCmdline(arch);
    setError(null);
    try {
      // Find the current architecture data
      const archData = data.architectures.find(a => a.architecture === arch);
      if (!archData) return;

      // Use createOsArchitecture which does an upsert
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
    <div className="space-y-4 max-w-4xl">
      <PageHeader
        breadcrumbs={[
          { label: "Operating Systems", href: "/operating-systems" },
          { label: `${data.name} ${data.version}` }
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
        <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
          {error}
        </div>
      )}

      {/* Basic Information */}
      <Card>
        <CardHeader>
          <CardTitle>Basic Information</CardTitle>
          <CardDescription>Operating system details</CardDescription>
        </CardHeader>
        <CardContent>
          {editingBasic ? (
            <form onSubmit={handleUpdateBasic} className="space-y-4">
              <FormField
                id="name"
                label="Name"
                required
                value={name}
                onChange={setName}
              />

              <FormField
                id="version"
                label="Version"
                required
                value={version}
                onChange={setVersion}
              />

              <FormTextareaField
                id="description"
                label="Description"
                value={description}
                onChange={setDescription}
                rows={3}
              />

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
                    setDescription(data.description || "");
                  }}
                >
                  Cancel
                </Button>
              </div>
            </form>
          ) : (
            <div className="space-y-2">
              <div><span className="font-medium">Name:</span> {data.name}</div>
              <div><span className="font-medium">Version:</span> {data.version}</div>
              <div>
                <span className="font-medium">Description:</span>{" "}
                {data.description || <span className="text-gray-400">—</span>}
              </div>
              <Button variant="outline" onClick={() => setEditingBasic(true)}>
                Edit
              </Button>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Architectures */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Architectures</CardTitle>
              <CardDescription>Configure boot files for different architectures</CardDescription>
            </div>
            <Dialog open={addArchDialogOpen} onOpenChange={setAddArchDialogOpen}>
              <DialogTrigger asChild>
                <Button>
                  <Plus className="h-4 w-4 mr-2" />
                  Add Architecture
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>Add Architecture</DialogTitle>
                  <DialogDescription>
                    Add support for a new architecture
                  </DialogDescription>
                </DialogHeader>
                <div className="space-y-4">
                  <div className="space-y-2">
                    <Label htmlFor="architecture">Architecture</Label>
                    <select
                      id="architecture"
                      value={newArchitecture}
                      onChange={(e) => setNewArchitecture(e.target.value as Architecture)}
                      className="w-full border rounded-md px-3 py-2"
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
        </CardHeader>
        <CardContent className="space-y-4">
          {data.architectures.length === 0 ? (
            <div className="text-center text-gray-400 py-8">
              No architectures configured. Add one to get started.
            </div>
          ) : (
            data.architectures.map((archData) => (
              <Card key={archData.architecture} className="border-2">
                <CardHeader>
                  <div className="flex items-center justify-between">
                    <button
                      onClick={() => toggleArchExpanded(archData.architecture)}
                      className="flex items-center gap-2 font-semibold text-lg hover:text-blue-600"
                    >
                      {expandedArchs[archData.architecture] ? (
                        <ChevronDown className="h-5 w-5" />
                      ) : (
                        <ChevronRight className="h-5 w-5" />
                      )}
                      <Badge variant="outline" className="text-lg px-3 py-1">
                        {archData.architecture}
                      </Badge>
                    </button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => openDeleteArchDialog(archData.architecture)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </CardHeader>

                {expandedArchs[archData.architecture] && (
                  <CardContent className="space-y-4">
                    {/* Kernel Upload */}
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

                    {/* Initramfs Upload */}
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
                    <div className="space-y-2">
                      <div className="font-medium">Modules:</div>
                      {archData.modules.length === 0 ? (
                        <div className="text-sm text-gray-400">No modules uploaded</div>
                      ) : (
                        <div className="space-y-2">
                          {archData.modules.map((module) => (
                            <div key={module} className="flex items-center justify-between border rounded px-3 py-2">
                              <span className="text-sm">{module}</span>
                              <div className="flex gap-2">
                                <Button
                                  variant="outline"
                                  size="sm"
                                  onClick={() => {
                                    window.location.href = getDownloadUrl(osId, archData.architecture, `modules/${module}`);
                                  }}
                                >
                                  Download
                                </Button>
                              </div>
                            </div>
                          ))}
                        </div>
                      )}
                      <Dialog open={moduleDialogOpen === archData.architecture} onOpenChange={(open) => setModuleDialogOpen(open ? archData.architecture : null)}>
                        <DialogTrigger asChild>
                          <Button variant="outline" size="sm">
                            <Plus className="h-4 w-4 mr-2" />
                            Upload Module
                          </Button>
                        </DialogTrigger>
                        <DialogContent>
                          <DialogHeader>
                            <DialogTitle>Upload Module</DialogTitle>
                            <DialogDescription>
                              Enter a name for the module and select the file to upload
                            </DialogDescription>
                          </DialogHeader>
                          <div className="space-y-4">
                            <div className="space-y-2">
                              <Label htmlFor="moduleName">Module Name</Label>
                              <Input
                                id="moduleName"
                                value={moduleName}
                                onChange={(e) => setModuleName(e.target.value)}
                                placeholder="e.g., network-driver.ko"
                              />
                            </div>
                            <FileUpload
                              label="Module File"
                              currentFile={undefined}
                              onUpload={(file) => handleUploadModule(archData.architecture, file)}
                            />
                          </div>
                        </DialogContent>
                      </Dialog>
                    </div>

                    {/* Install Script Upload */}
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
                          window.location.href = getDownloadUrl(osId, archData.architecture, "install_script");
                        }}
                      />
                      <TemplateDocs type="install-script" />
                    </div>

                    {/* Cmdline Args */}
                    <div className="space-y-3">
                      <div className="space-y-2">
                        <Label htmlFor={`cmdline-${archData.architecture}`}>
                          Kernel Command Line Arguments
                        </Label>
                        <Textarea
                          id={`cmdline-${archData.architecture}`}
                          value={cmdlineArgs[archData.architecture] || ""}
                          onChange={(e) => setCmdlineArgs(prev => ({ ...prev, [archData.architecture]: e.target.value }))}
                          placeholder="Additional kernel boot parameters"
                          rows={2}
                        />
                        <Button
                          size="sm"
                          onClick={() => handleSaveCmdlineArgs(archData.architecture)}
                          disabled={savingCmdline === archData.architecture}
                          className="bg-green-600 hover:bg-green-700 text-white"
                        >
                          {savingCmdline === archData.architecture ? "Saving..." : "Save"}
                        </Button>
                      </div>
                      <TemplateDocs type="cmdline" />
                    </div>
                  </CardContent>
                )}
              </Card>
            ))
          )}
        </CardContent>
      </Card>

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
