'use client'
import Image from "next/image";
import VersionPane from "@/app/_components/versionPane";

export default function Home() {
  return (
      <main className="flex min-h-screen flex-col items-center justify-between p-10">
          <VersionPane/>
          <div className="absolute bottom-10 left-0 right-0 text-center text-gray-700 dark:text-white">
              Made with <a className="font-medium text-blue-600 underline dark:text-blue-500 hover:no-underline"
                           href="https://tauri.app/" target="_blank">Tauri</a> and <a
              className="font-medium text-blue-600 underline dark:text-blue-500 hover:no-underline"
              href="https://www.rust-lang.org/" target="_blank">Rust</a> on macOS for windows. (Erm maybe linux also...)
              <div className="text-gray-300 dark:text-gray-600">No servers were harmed when checking build version. I think...should be la.</div>
          </div>
      </main>
  );
}
