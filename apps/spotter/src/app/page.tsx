'use client'
import Image from "next/image";
import VersionPane from "@/app/_components/versionPane";

export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-between p-20">
      <VersionPane />
    </main>
  );
}
