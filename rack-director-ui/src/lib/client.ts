import { redirect } from "react-router";

export type Plan = {

}

export type Device = {
  uuid: String,
  hostname: String,
  plan: Plan | null,
}

export type DevicesIndex = {
  devices: Device[]
}

export async function getDevicesIndex(): Promise<DevicesIndex> {
  return fetch('/ui/devices').then((response) => {
    if (response.ok) {
      return response.json()
    } else {
      console.log('Error getting /ui/devices:', response.statusText)
      return redirect('error')
    }
  });
}
