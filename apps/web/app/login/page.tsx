import Link from "next/link";
import { AuthForm } from "@/components/AuthForm";

export const metadata = { title: "Sign in · Sprintly" };

export default function LoginPage() {
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-8 px-6 py-20">
      <header className="space-y-2">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · auth
        </div>
        <h1 className="text-3xl font-semibold">Welcome back.</h1>
        <p className="text-sm text-chrome-dim">
          Sign in to your self-hosted instance.
        </p>
      </header>

      <AuthForm mode="login" />

      <footer className="mono text-xs text-chrome-dim">
        no account?{" "}
        <Link href="/register" className="text-accent hover:underline">
          register here
        </Link>
      </footer>
    </main>
  );
}
