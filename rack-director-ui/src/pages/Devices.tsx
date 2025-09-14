import DevicesTable from "@/components/devices/devices-table";
import type { DevicesIndex } from "@/lib/client";
import { useLoaderData } from "react-router"



function Devices() {
  const data = useLoaderData<DevicesIndex>();
  return (
    <>
      <h1>Rack Director</h1>
      <DevicesTable data={data.devices} />
    </>
  )
}

export default Devices
