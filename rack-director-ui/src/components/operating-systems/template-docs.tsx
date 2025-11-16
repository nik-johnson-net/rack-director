import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

interface TemplateDocsProps {
  type: 'install-script' | 'cmdline';
}

export default function TemplateDocs({ type }: TemplateDocsProps) {
  if (type === 'cmdline') {
    return (
      <Card className="bg-blue-50 border-blue-200">
        <CardHeader>
          <CardTitle className="text-sm">Available Template Variables</CardTitle>
          <CardDescription className="text-xs">
            Use Handlebars syntax to reference these variables
          </CardDescription>
        </CardHeader>
        <CardContent className="text-sm space-y-2">
          <div>
            <Badge variant="secondary" className="font-mono text-xs">
              {"{{ install_script_url }}"}
            </Badge>
            <span className="ml-2 text-gray-600">- URL to fetch the install script</span>
          </div>
          <div className="text-xs text-gray-500 mt-3">
            <strong>Example:</strong>
            <pre className="mt-1 bg-white p-2 rounded border text-xs overflow-x-auto">
              autoinstall ds=nocloud-net;s={"{{ install_script_url }}"} console=ttyS0
            </pre>
          </div>
        </CardContent>
      </Card>
    );
  }

  // install-script type
  return (
    <Card className="bg-blue-50 border-blue-200">
      <CardHeader>
        <CardTitle className="text-sm">Available Template Variables</CardTitle>
        <CardDescription className="text-xs">
          Use Handlebars syntax ({"{{ variable }}"}) to reference these in your install script
        </CardDescription>
      </CardHeader>
      <CardContent className="text-sm space-y-3">
        <div>
          <div className="font-semibold text-gray-700 mb-1">Device Information:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.uuid }}"}</Badge>
              <span className="ml-2 text-gray-600">- Device UUID</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.hostname }}"}</Badge>
              <span className="ml-2 text-gray-600">- Device hostname</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.mac_address }}"}</Badge>
              <span className="ml-2 text-gray-600">- Primary MAC address</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.ip_address }}"}</Badge>
              <span className="ml-2 text-gray-600">- IP address (DHCP lease)</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.gateway }}"}</Badge>
              <span className="ml-2 text-gray-600">- Network gateway</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.dns_servers }}"}</Badge>
              <span className="ml-2 text-gray-600">- DNS servers (space-separated)</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ device.netmask }}"}</Badge>
              <span className="ml-2 text-gray-600">- Network netmask</span>
            </div>
          </div>
        </div>

        <div>
          <div className="font-semibold text-gray-700 mb-1">Role Information:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ role.name }}"}</Badge>
              <span className="ml-2 text-gray-600">- Role name</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ role.disk_layout }}"}</Badge>
              <span className="ml-2 text-gray-600">- Disk layout as JSON</span>
            </div>
          </div>
        </div>

        <div>
          <div className="font-semibold text-gray-700 mb-1">Operating System:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ os.name }}"}</Badge>
              <span className="ml-2 text-gray-600">- OS name</span>
            </div>
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ os.version }}"}</Badge>
              <span className="ml-2 text-gray-600">- OS version</span>
            </div>
          </div>
        </div>

        <div>
          <div className="font-semibold text-gray-700 mb-1">Custom Configuration:</div>
          <div className="space-y-1 ml-2">
            <div>
              <Badge variant="secondary" className="font-mono text-xs">{"{{ config.* }}"}</Badge>
              <span className="ml-2 text-gray-600">- Any custom config from role.config_template</span>
            </div>
          </div>
        </div>

        <div className="text-xs text-gray-500 mt-3">
          <strong>Example (Debian Preseed):</strong>
          <pre className="mt-1 bg-white p-2 rounded border text-xs overflow-x-auto">
{`d-i netcfg/get_hostname string {{ device.hostname }}
d-i netcfg/get_ipaddress string {{ device.ip_address }}
d-i netcfg/get_netmask string {{ device.netmask }}
d-i netcfg/get_gateway string {{ device.gateway }}
d-i netcfg/get_nameservers string {{ device.dns_servers }}`}
          </pre>
        </div>
      </CardContent>
    </Card>
  );
}
