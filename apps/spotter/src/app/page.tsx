'use client'
import Image from "next/image";
import VersionPane from "@/app/_components/versionPane";

export default function Home() {
  return (
      <main className="flex min-h-screen flex-col items-center justify-between p-20">
          <VersionPane/>
          <div className="absolute bottom-10 left-0 right-0 text-center">
              Made with <a className="font-medium text-blue-600 underline dark:text-blue-500 hover:no-underline" href="https://tauri.app/">Tauri</a> and <a className="font-medium text-blue-600 underline dark:text-blue-500 hover:no-underline" href="https://www.rust-lang.org/">Rust</a> :)
          </div>
      </main>
  );
}
