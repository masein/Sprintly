import Link from "next/link";
import { LoginPanel } from "@/components/LoginPanel";

export const metadata = { title: "Sign in · Sprintly" };

export default function LoginPage({
  searchParams,
}: {
  searchParams?: { sso_error?: string };
}) {
  const ssoError = searchParams?.sso_error ?? null;
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

      <LoginPanel ssoError={ssoError} />

      <footer className="mono text-xs text-chrome-dim">
        no account?{" "}
        <Link href="/register" className="text-accent hover:underline">
          register here
        </Link>
      </footer>
    </main>
  );
}
