import DevicesTable from "@/components/devices/devices-table";
import type { DevicesIndex } from "@/lib/client";
import { useLoaderData } from "react-router"



function Devices() {
  const data = useLoaderData<DevicesIndex>();
  return (
    <>
      <DevicesTable data={data.devices} />
    </>
  )
}

export default Devices
